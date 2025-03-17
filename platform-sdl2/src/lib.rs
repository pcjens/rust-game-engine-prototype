// SPDX-FileCopyrightText: 2024 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, RefCell},
    ffi::{c_int, c_void},
    fmt::Arguments,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    ops::ControlFlow,
    panic,
    path::PathBuf,
    process::exit,
    ptr::{addr_of, null_mut},
    str::FromStr,
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

use platform::{
    ActionCategory, Button, DrawSettings2D, EngineCallbacks, FileHandle, FileReadTask, InputDevice,
    InputDevices, Platform, Vertex2D, AUDIO_CHANNELS, AUDIO_SAMPLE_RATE,
};
use sdl2::{
    audio::{AudioCallback, AudioDevice, AudioSpec, AudioSpecDesired},
    controller::Button as SdlButton,
    event::Event,
    keyboard::{Keycode, Mod, Scancode},
    pixels::{Color, PixelFormatEnum},
    rect::Rect,
    render::{Texture, TextureAccess, TextureCreator, WindowCanvas},
    video::WindowContext,
    AudioSubsystem, Sdl, TimerSubsystem,
};
use sdl2_sys::{
    SDL_BlendMode, SDL_Color, SDL_GameController, SDL_GameControllerGetType,
    SDL_GameControllerOpen, SDL_GameControllerType, SDL_GetTicks64, SDL_RenderGeometryRaw,
    SDL_Renderer, SDL_ScaleMode, SDL_SetTextureBlendMode, SDL_SetTextureScaleMode,
};

enum Hid {
    Keyboard,
    Gamepad {
        /// An opened SDL game controller which we'll never close. Unfortunate,
        /// but shouldn't cause any major issues.
        controller: *mut SDL_GameController,
        connected: bool,
        instance_id: u32,
    },
}

type FileReadHandle = JoinHandle<Result<Vec<u8>, io::Error>>;

struct FileHolder {
    path: PathBuf,
    tasks: Vec<(u64, FileReadHandle)>,
    task_id_counter: u64,
}

struct AudioBufferState {
    /// The first audio playback position that will be played back in the next
    /// audio callback.
    position: u64,
    /// The timestamp matching the `position`.
    sync_timestamp: platform::Instant,
    /// The internal buffer of the samples to be played back, starting at
    /// the audio playback position in the `position` field.
    buffer: Vec<[i16; AUDIO_CHANNELS]>,
}

type SharedAudioBuffer = Arc<Mutex<AudioBufferState>>;

/// The [`Platform`] impl for the SDL2 based platform.
pub struct Sdl2Platform {
    sdl_context: Sdl,
    _time: TimerSubsystem,
    _audio: AudioSubsystem,
    audio_device: Option<AudioDevice<AudioCallbackImpl>>,
    canvas: RefCell<WindowCanvas>,
    exit_requested: Cell<bool>,
    texture_creator: &'static TextureCreator<WindowContext>,
    textures: RefCell<Vec<Texture<'static>>>,
    /// List of input devices. Devices are never removed, so the InputDevice ids
    /// used for this platform are indices to this list.
    hids: RefCell<Vec<Hid>>,
    files: RefCell<Vec<FileHolder>>,
    shared_audio_buffer: SharedAudioBuffer,
}

impl Drop for Sdl2Platform {
    fn drop(&mut self) {
        if let Some(audio_device) = self.audio_device.take() {
            // Letting the AudioDevice drop normally seems to segfault. The
            // issue seems to be that the user data in the audio device contains
            // a mutex, and some glibc mutex assert is being tripped. This way
            // we get the mutex (in AudioCallbackImpl) back to Rust-land, to be
            // dropped as it should.
            audio_device.close_and_get_callback();
        }
    }
}

impl Sdl2Platform {
    pub fn new(title: &str) -> Sdl2Platform {
        let sdl_context = sdl2::init().expect("SDL 2 library should be able to init");

        let video = sdl_context
            .video()
            .expect("SDL video subsystem should be able to init");
        let window = video
            .window(title, 960, 540)
            .allow_highdpi()
            .position_centered()
            .resizable()
            .build()
            .expect("should be able to create a window");
        let canvas = window
            .into_canvas()
            .present_vsync()
            .build()
            .expect("should be able to create a renderer");

        let time = sdl_context
            .timer()
            .expect("SDL timer subsystem should be able to init");

        let texture_creator = Box::leak(Box::new(canvas.texture_creator()));

        let audio = sdl_context
            .audio()
            .expect("SDL audio subsystem should be able to init");

        let shared_audio_buffer = Arc::new(Mutex::new(AudioBufferState {
            position: 0,
            sync_timestamp: current_time(),
            buffer: Vec::new(),
        }));
        let audio_device = match audio.open_playback(
            None,
            &AudioSpecDesired {
                freq: Some(AUDIO_SAMPLE_RATE as i32),
                channels: Some(2),
                samples: None,
            },
            |spec| AudioCallbackImpl::new(spec, shared_audio_buffer.clone()),
        ) {
            Ok(device) => {
                device.resume();
                Some(device)
            }
            Err(err) => {
                eprintln!("Failed to open audio device, continuing without playback: {err}");
                None
            }
        };

        Sdl2Platform {
            sdl_context,
            _time: time,
            _audio: audio,
            audio_device,
            canvas: RefCell::new(canvas),
            exit_requested: Cell::new(false),
            texture_creator,
            textures: RefCell::new(Vec::new()),
            hids: RefCell::new(vec![Hid::Keyboard]),
            files: RefCell::new(Vec::new()),
            shared_audio_buffer,
        }
    }

    fn get_input_device_by_sdl_joystick_id(&self, which: u32) -> Option<InputDevice> {
        let hids = self.hids.borrow();
        for (i, hid) in hids.iter().enumerate() {
            if let Hid::Gamepad {
                connected: true,
                instance_id,
                ..
            } = hid
            {
                if *instance_id == which {
                    return Some(InputDevice::new(i as u64));
                }
            }
        }
        None
    }

    pub fn run_game_loop<I, A>(
        &self,
        engine: &mut dyn EngineCallbacks<InitParams = I, Arena = A>,
        mut game_init_arena: A,
        game_init_params: I,
    ) {
        // Init the subsystem. The subsystem is actually used, just through the FFI
        // calls, since the subsystem doesn't expose everything we need (e.g. game
        // controller type).
        let _gamepad = self
            .sdl_context
            .game_controller()
            .expect("SDL controller subsystem should be able to init");
        let mut event_pump = self
            .sdl_context
            .event_pump()
            .expect("SDL 2 event pump should init without issue");

        let mut init_params = Some(game_init_params);
        'init_loop: while !self.exit_requested.get() {
            if let Some(init_params) = init_params.take() {
                engine.init(init_params, &mut game_init_arena);
            }

            'game_loop: while !self.exit_requested.get() {
                for event in event_pump.poll_iter() {
                    match event {
                        Event::Quit { .. } => {
                            self.exit_requested.set(true);
                            thread::spawn(|| {
                                // Force-exit the process after 1s, cleanup is pretty optional anyway.
                                thread::sleep(Duration::from_secs(1));
                                eprintln!(
                                    "Resource cleanup is taking too long, exiting non-gracefully."
                                );
                                std::process::exit(1);
                            });
                        }
                        Event::KeyDown {
                            keycode: Some(Keycode::Q),
                            keymod,
                            ..
                        } if keymod.intersects(Mod::LCTRLMOD) => {
                            self.exit_requested.set(true);
                        }

                        Event::ControllerDeviceAdded { which, .. } => {
                            // Safety: ffi call.
                            let controller = unsafe { SDL_GameControllerOpen(which as i32) };
                            if !controller.is_null() {
                                let mut hids = self.hids.borrow_mut();
                                hids.push(Hid::Gamepad {
                                    controller,
                                    connected: true,
                                    instance_id: which,
                                });
                            }
                        }
                        Event::ControllerDeviceRemoved { which, .. } => {
                            let mut hids = self.hids.borrow_mut();
                            for hid in hids.iter_mut() {
                                if let Hid::Gamepad {
                                    connected,
                                    instance_id,
                                    ..
                                } = hid
                                {
                                    if *connected && *instance_id == which {
                                        *connected = false;
                                        break;
                                    }
                                }
                            }
                        }

                        Event::KeyDown {
                            timestamp,
                            scancode: Some(scancode),
                            ..
                        } => {
                            engine.event(
                                platform::Event::DigitalInputPressed(
                                    InputDevice::new(0),
                                    button_for_scancode(scancode),
                                ),
                                platform::Instant::reference()
                                    + Duration::from_millis(timestamp as u64),
                            );
                        }

                        Event::KeyUp {
                            timestamp,
                            scancode: Some(scancode),
                            ..
                        } => {
                            engine.event(
                                platform::Event::DigitalInputReleased(
                                    InputDevice::new(0),
                                    button_for_scancode(scancode),
                                ),
                                platform::Instant::reference()
                                    + Duration::from_millis(timestamp as u64),
                            );
                        }

                        Event::ControllerButtonDown {
                            timestamp,
                            which,
                            button,
                        } => {
                            if let Some(device) = self.get_input_device_by_sdl_joystick_id(which) {
                                engine.event(
                                    platform::Event::DigitalInputPressed(
                                        device,
                                        button_for_gamepad(button),
                                    ),
                                    platform::Instant::reference()
                                        + Duration::from_millis(timestamp as u64),
                                );
                            }
                        }

                        Event::ControllerButtonUp {
                            timestamp,
                            which,
                            button,
                        } => {
                            if let Some(device) = self.get_input_device_by_sdl_joystick_id(which) {
                                engine.event(
                                    platform::Event::DigitalInputReleased(
                                        device,
                                        button_for_gamepad(button),
                                    ),
                                    platform::Instant::reference()
                                        + Duration::from_millis(timestamp as u64),
                                );
                            }
                        }

                        _ => {}
                    }
                }

                {
                    let mut canvas = self.canvas.borrow_mut();
                    canvas.set_draw_color(Color::BLACK);
                    canvas.clear();
                }

                match engine.run_frame(self) {
                    ControlFlow::Continue(()) => {}
                    ControlFlow::Break(Some(new_init_params)) => {
                        init_params = Some(new_init_params);
                        break 'game_loop;
                    }
                    ControlFlow::Break(None) => break 'init_loop,
                }

                {
                    let mut canvas = self.canvas.borrow_mut();
                    canvas.present();
                }
            }
        }
    }
}

impl Platform for Sdl2Platform {
    fn draw_area(&self) -> (f32, f32) {
        let (w, h) = {
            let canvas = self.canvas.borrow();
            canvas.window().size()
        };
        (w as f32, h as f32)
    }

    fn draw_scale_factor(&self) -> f32 {
        let (scaled_width, pixel_width) = {
            let canvas = self.canvas.borrow();
            let (scaled_w, _) = canvas.window().size();
            let (pixel_w, _) = canvas.window().drawable_size();
            (scaled_w, pixel_w)
        };
        pixel_width as f32 / scaled_width as f32
    }

    fn draw_2d(&self, vertices: &[Vertex2D], indices: &[u32], settings: DrawSettings2D) {
        if vertices.len() < 3 || indices.len() < 3 {
            return;
        }

        let xy_ptr = addr_of!(vertices[0].x);
        let uv_ptr = addr_of!(vertices[0].u);
        let rgba_ptr = addr_of!(vertices[0].r) as *const SDL_Color;
        let stride = size_of_val(&vertices[0]) as c_int;
        let num_vertices = vertices.len() as c_int;

        let indices_ptr = indices.as_ptr() as *const c_void;
        let index_size = size_of_val(&indices[0]) as c_int;
        let num_indices = indices.len() as c_int;

        {
            let mut canvas = self.canvas.borrow_mut();
            let textures = self.textures.borrow();

            // Update draw settings
            let clip_rect = settings
                .clip_area
                .map(|[x, y, w, h]| Rect::new(x as i32, y as i32, w as u32, h as u32));
            let blend_mode = match settings.blend_mode {
                platform::BlendMode::None => SDL_BlendMode::SDL_BLENDMODE_NONE,
                platform::BlendMode::Blend => SDL_BlendMode::SDL_BLENDMODE_BLEND,
                platform::BlendMode::Add => SDL_BlendMode::SDL_BLENDMODE_ADD,
            };
            let scale_mode = match settings.texture_filter {
                platform::TextureFilter::NearestNeighbor => SDL_ScaleMode::SDL_ScaleModeNearest,
                platform::TextureFilter::Linear => SDL_ScaleMode::SDL_ScaleModeLinear,
            };
            let texture = if let Some(texture_index) = settings.sprite {
                let i = texture_index.inner() as usize;
                if i < textures.len() {
                    textures[i].raw()
                } else {
                    null_mut()
                }
            } else {
                null_mut()
            };

            canvas.set_clip_rect(clip_rect);

            // Safety: ffi call.
            unsafe { SDL_SetTextureScaleMode(texture, scale_mode) };

            // Safety: ffi call.
            unsafe { SDL_SetTextureBlendMode(texture, blend_mode) };

            let renderer: *mut SDL_Renderer = canvas.raw();
            // Safety: ffi call.
            unsafe {
                SDL_RenderGeometryRaw(
                    renderer,
                    texture,
                    xy_ptr,
                    stride,
                    rgba_ptr,
                    stride,
                    uv_ptr,
                    stride,
                    num_vertices,
                    indices_ptr,
                    num_indices,
                    index_size,
                );
            }
        }
    }

    fn create_sprite(
        &self,
        width: u16,
        height: u16,
        format: platform::PixelFormat,
    ) -> Option<platform::SpriteRef> {
        let fmt = match format {
            // Unsure why ABGR8888 reads `[r, g, b, a, r, ...]` correctly, but here we are.
            platform::PixelFormat::Rgba => PixelFormatEnum::ABGR8888,
        };
        let texture = self
            .texture_creator
            .create_texture(fmt, TextureAccess::Streaming, width as u32, height as u32)
            .ok()?;
        let texture_index = {
            let mut textures = self.textures.borrow_mut();
            let idx = textures.len();
            textures.push(texture);
            idx
        };
        Some(platform::SpriteRef::new(texture_index as u64))
    }

    fn update_sprite(
        &self,
        texture: platform::SpriteRef,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        pixels: &[u8],
    ) {
        let mut textures = self.textures.borrow_mut();
        if let Some(tex) = textures.get_mut(texture.inner() as usize) {
            let bpp = tex.query().format.byte_size_per_pixel();
            if let Err(err) = tex.update(
                Rect::new(x as i32, y as i32, width as u32, height as u32),
                pixels,
                width as usize * bpp,
            ) {
                println!("[Sdl2Platform::update_sprite]: sprite update failed: {err}");
            }
        }
    }

    fn open_file(&self, path: &str) -> Option<FileHandle> {
        let handle = {
            let path = PathBuf::from_str(path).ok()?;
            if !path.exists() {
                return None;
            }
            let mut files = self.files.borrow_mut();
            let i = files.len() as u64;
            files.push(FileHolder {
                path,
                tasks: Vec::new(),
                task_id_counter: 0,
            });
            FileHandle::new(i)
        };
        Some(handle)
    }

    fn begin_file_read(
        &self,
        file: FileHandle,
        first_byte: u64,
        buffer: platform::Box<[u8]>,
    ) -> FileReadTask {
        // This is not an efficient implementation, it's a proof of concept.
        let id = {
            let mut files = self.files.borrow_mut();
            let file = files
                .get_mut(file.inner() as usize)
                .expect("invalid FileHandle");
            let id = file.task_id_counter;
            file.task_id_counter += 1;
            let path = file.path.clone();
            let mut buffer_on_thread = vec![0; buffer.len()];
            file.tasks.push((
                id,
                std::thread::spawn(move || {
                    let mut file = File::open(path)?;
                    file.seek(SeekFrom::Start(first_byte))?;
                    file.read_exact(&mut buffer_on_thread)?;
                    Ok(buffer_on_thread)
                }),
            ));
            id
        };
        FileReadTask::new(file, id, buffer)
    }

    fn is_file_read_finished(&self, task: &FileReadTask) -> bool {
        let files = self.files.borrow();
        let file = files
            .get(task.file().inner() as usize)
            .expect("invalid FileHandle");
        let Some(idx) = file.tasks.iter().position(|(id, _)| *id == task.task_id()) else {
            panic!("tried to poll a read task with an invalid task id?");
        };
        file.tasks[idx].1.is_finished()
    }

    fn finish_file_read(
        &self,
        task: FileReadTask,
    ) -> Result<platform::Box<[u8]>, platform::Box<[u8]>> {
        let written_buffer = {
            let mut files = self.files.borrow_mut();
            let file = files
                .get_mut(task.file().inner() as usize)
                .expect("invalid FileHandle");
            let Some(idx) = file.tasks.iter().position(|(id, _)| *id == task.task_id()) else {
                panic!("tried to finish a read task with an invalid task id?");
            };

            let (_, join_handle) = file.tasks.swap_remove(idx);

            // Safety: this implementation does not share the borrow in the first place.
            let mut buffer = unsafe { task.into_inner() };

            match join_handle.join().unwrap() {
                Ok(read_bytes) => {
                    buffer.copy_from_slice(&read_bytes);
                    buffer
                }
                Err(err) => {
                    println!("[Sdl2Platform::finish_file_read]: could not read file: {err}");
                    return Err(buffer);
                }
            }
        };
        Ok(written_buffer)
    }

    fn create_semaphore(&self) -> platform::Semaphore {
        struct Semaphore {
            value: Mutex<u32>,
            condvar: Condvar,
        }

        impl platform::SemaphoreImpl for Semaphore {
            fn increment(&self) {
                let mut value_lock = self.value.lock().unwrap();
                *value_lock += 1;
                self.condvar.notify_one();
            }

            fn decrement(&self) {
                let mut value_lock = self.value.lock().unwrap();
                while *value_lock == 0 {
                    value_lock = self.condvar.wait(value_lock).unwrap();
                }
                *value_lock -= 1;
            }
        }

        let semaphore: &'static mut Semaphore = Box::leak(Box::new(Semaphore {
            value: Mutex::new(0),
            condvar: Condvar::new(),
        }));

        // Safety: the semaphore is definitely valid for the entire lifetime of
        // the semaphore, since we have a static borrow of it.
        unsafe { platform::Semaphore::new(semaphore, None) }
    }

    fn available_parallelism(&self) -> usize {
        thread::available_parallelism()
            .map(|u| u.get())
            .unwrap_or(1)
    }

    fn spawn_pool_thread(&self, channels: [platform::TaskChannel; 2]) -> platform::ThreadState {
        let [(task_sender, mut task_receiver), (mut result_sender, result_receiver)] = channels;
        thread::Builder::new()
            .name("threadpool".to_string())
            .spawn(move || loop {
                let mut task = task_receiver.recv();

                let result = panic::catch_unwind(panic::AssertUnwindSafe(|| task.run()));
                if result.is_err() {
                    // Signal the main thread that we panicked, but don't resume
                    // until we've sent this information back.
                    task.signal_panic();
                }

                'send_result: loop {
                    match result_sender.send(task) {
                        Ok(()) => break 'send_result,
                        Err(task_) => {
                            thread::sleep(Duration::from_millis(1));
                            task = task_;
                        }
                    }
                }

                if let Err(err) = result {
                    // We can now resume panicking without causing a hang.
                    panic::resume_unwind(err);
                }
            })
            .unwrap();
        platform::ThreadState::new(task_sender, result_receiver)
    }

    fn update_audio_buffer(&self, first_position: u64, mut samples: &[[i16; AUDIO_CHANNELS]]) {
        let mut shared = self.shared_audio_buffer.lock().unwrap();
        let played_position = shared.position;
        let dst_samples = &mut shared.buffer;

        if first_position > played_position {
            let not_provided_samples = (first_position - played_position) as usize;
            let old_samples_to_use = not_provided_samples.min(dst_samples.len());
            dst_samples.truncate(old_samples_to_use);
            let missing_samples = not_provided_samples - old_samples_to_use;
            // Fill the samples between the engine's previously provided buffer
            // and this new one with silence (should be relatively rare)
            for _ in 0..missing_samples {
                dst_samples.push([0; AUDIO_CHANNELS]);
            }
        } else {
            dst_samples.clear();
        }

        if played_position > first_position {
            let already_played_samples = (played_position - first_position) as usize;
            let start = already_played_samples.min(samples.len());
            samples = &samples[start..];
        }

        dst_samples.extend_from_slice(samples);
    }

    fn audio_playback_position(&self) -> (u64, platform::Instant) {
        // Offset the playback position forwards enough that any new sounds
        // played by the engine don't start too early (which would pop)
        let latency_offset = {
            let canvas = self.canvas.borrow();
            let fps = (canvas.window().display_mode().map(|dm| dm.refresh_rate)).unwrap_or(60);
            AUDIO_SAMPLE_RATE as u64 / fps as u64
        };

        let audio_buffer = self.shared_audio_buffer.lock().unwrap();
        (
            audio_buffer.position + latency_offset,
            audio_buffer.sync_timestamp,
        )
    }

    fn input_devices(&self) -> InputDevices {
        let mut devices = InputDevices::new();
        {
            let hids = self.hids.borrow();
            for (id, hid) in hids.iter().enumerate() {
                if let Hid::Gamepad {
                    connected: false, ..
                } = hid
                {
                    continue;
                }
                devices.push(InputDevice::new(id as u64));
            }
        }
        devices
    }

    fn default_button_for_action(
        &self,
        action: ActionCategory,
        device: InputDevice,
    ) -> Option<Button> {
        let hids = self.hids.borrow();
        if let Some(hid) = hids.get(device.inner() as usize) {
            let button = match hid {
                Hid::Keyboard => match action {
                    ActionCategory::Up => button_for_scancode(Scancode::Up),
                    ActionCategory::Down => button_for_scancode(Scancode::Down),
                    ActionCategory::Right => button_for_scancode(Scancode::Right),
                    ActionCategory::Left => button_for_scancode(Scancode::Left),
                    ActionCategory::Accept => button_for_scancode(Scancode::X),
                    ActionCategory::Cancel => button_for_scancode(Scancode::Z),
                    ActionCategory::Jump => button_for_scancode(Scancode::Space),
                    ActionCategory::Run => button_for_scancode(Scancode::LShift),
                    ActionCategory::ActPrimary => button_for_scancode(Scancode::X),
                    ActionCategory::ActSecondary => button_for_scancode(Scancode::Z),
                    ActionCategory::Pause => button_for_scancode(Scancode::Escape),
                },
                Hid::Gamepad { controller, .. } => match action {
                    ActionCategory::Up => button_for_gamepad(SdlButton::DPadUp),
                    ActionCategory::Down => button_for_gamepad(SdlButton::DPadDown),
                    ActionCategory::Right => button_for_gamepad(SdlButton::DPadRight),
                    ActionCategory::Left => button_for_gamepad(SdlButton::DPadLeft),
                    ActionCategory::Accept => {
                        button_for_gamepad(if flip_accept_cancel(*controller) {
                            SdlButton::B // the right face button
                        } else {
                            SdlButton::A // the bottom face button
                        })
                    }
                    ActionCategory::Cancel => {
                        button_for_gamepad(if flip_accept_cancel(*controller) {
                            SdlButton::A // the bottom face button
                        } else {
                            SdlButton::B // the right face button
                        })
                    }
                    ActionCategory::Jump => button_for_gamepad(SdlButton::A),
                    ActionCategory::Run => button_for_gamepad(SdlButton::B),
                    ActionCategory::ActPrimary => button_for_gamepad(SdlButton::X),
                    ActionCategory::ActSecondary => button_for_gamepad(SdlButton::Y),
                    ActionCategory::Pause => button_for_gamepad(SdlButton::Start),
                },
            };
            Some(button)
        } else {
            None
        }
    }

    fn now(&self) -> platform::Instant {
        current_time()
    }

    fn println(&self, message: Arguments) {
        println!("[Sdl2Platform::println]: {message}");
    }

    fn exit(&self, clean: bool) {
        if !clean {
            exit(1);
        }
        self.exit_requested.set(true);
    }
}

// Timing helper:

fn current_time() -> platform::Instant {
    // Not using Instant even though we have std, to make timestamps between SDL
    // events and this consistent.
    // Safety: ffi call of a function without any special safety invariants, at
    // least according to the docs. Should be fine.
    platform::Instant::reference() + Duration::from_millis(unsafe { SDL_GetTicks64() })
}

// Keyboard/gamepad input helpers:

fn flip_accept_cancel(controller: *mut SDL_GameController) -> bool {
    // Safety: controller is not null (checked when we get the pointer from an
    // event).
    let controller_type = unsafe { SDL_GameControllerGetType(controller) };
    matches!(
        controller_type,
        SDL_GameControllerType::SDL_CONTROLLER_TYPE_NINTENDO_SWITCH_PRO
            | SDL_GameControllerType::SDL_CONTROLLER_TYPE_NINTENDO_SWITCH_JOYCON_LEFT
            | SDL_GameControllerType::SDL_CONTROLLER_TYPE_NINTENDO_SWITCH_JOYCON_RIGHT
            | SDL_GameControllerType::SDL_CONTROLLER_TYPE_NINTENDO_SWITCH_JOYCON_PAIR
    )
}

fn button_for_scancode(scancode: Scancode) -> Button {
    Button::new((1 << 32) | scancode as u64)
}

fn button_for_gamepad(gamepad_button: SdlButton) -> Button {
    Button::new((2 << 32) | gamepad_button as u64)
}

// Audio helpers:

struct AudioCallbackImpl {
    shared_audio_buffer: SharedAudioBuffer,
}

impl AudioCallbackImpl {
    fn new(spec: AudioSpec, shared_audio_buffer: SharedAudioBuffer) -> AudioCallbackImpl {
        assert_eq!(
            AUDIO_SAMPLE_RATE as i32, spec.freq,
            "platform-sdl2 doesn't support resampling audio",
        );

        assert_eq!(
            AUDIO_CHANNELS as u8, spec.channels,
            "platform-sdl2 doesn't support resampling audio",
        );

        AudioCallbackImpl {
            shared_audio_buffer,
        }
    }
}

impl AudioCallback for AudioCallbackImpl {
    type Channel = i16;
    fn callback(&mut self, dst_samples: &mut [Self::Channel]) {
        let mut src = self.shared_audio_buffer.lock().unwrap();
        let src_samples = &src.buffer;

        let mut samples_played_back = 0;
        for (src, dst) in src_samples
            .iter()
            .zip(dst_samples.chunks_exact_mut(AUDIO_CHANNELS))
        {
            dst.copy_from_slice(src);
            samples_played_back += 1;
        }

        src.buffer.splice(0..samples_played_back as usize, None);

        let leftover_dst = &mut dst_samples[samples_played_back as usize * 2..];
        if !leftover_dst.is_empty() {
            leftover_dst.fill(0);
            samples_played_back += leftover_dst.len() as u64 / 2;
        }

        src.position += samples_played_back;
        src.sync_timestamp = current_time();
    }
}
