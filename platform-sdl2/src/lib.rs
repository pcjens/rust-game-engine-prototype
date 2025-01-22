use std::{
    cell::{Cell, RefCell},
    ffi::{c_int, c_void},
    fmt::Arguments,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::PathBuf,
    process::exit,
    ptr::{addr_of, null_mut},
    str::FromStr,
    thread::{self, JoinHandle},
    time::Duration,
};

use platform_abstraction_layer::{
    self as pal, ActionCategory, Button, DrawSettings, EngineCallbacks, FileHandle, FileReadTask,
    InputDevice, InputDevices, Pal, Vertex,
};
use sdl2::{
    controller::Button as SdlButton,
    event::Event,
    keyboard::{Keycode, Mod, Scancode},
    pixels::{Color, PixelFormatEnum},
    rect::Rect,
    render::{Texture, TextureAccess, TextureCreator, WindowCanvas},
    video::WindowContext,
    Sdl, TimerSubsystem,
};
use sdl2_sys::{
    SDL_BlendMode, SDL_Color, SDL_GameController, SDL_GameControllerGetType,
    SDL_GameControllerOpen, SDL_GameControllerType, SDL_RenderGeometryRaw, SDL_Renderer,
    SDL_ScaleMode, SDL_SetTextureBlendMode, SDL_SetTextureScaleMode, SDL_free, SDL_malloc,
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
}

/// The [`Pal`] impl for the SDL2 based platform.
pub struct Sdl2Pal {
    sdl_context: Sdl,
    time: TimerSubsystem,
    canvas: RefCell<WindowCanvas>,
    exit_requested: Cell<bool>,
    texture_creator: &'static TextureCreator<WindowContext>,
    textures: RefCell<Vec<Texture<'static>>>,
    /// List of input devices. Devices are never removed, so the InputDevice ids
    /// used for this platform are indices to this list.
    hids: RefCell<Vec<Hid>>,
    files: RefCell<Vec<FileHolder>>,
}

impl Sdl2Pal {
    pub fn new() -> Sdl2Pal {
        let sdl_context = sdl2::init().expect("SDL 2 library should be able to init");

        let video = sdl_context
            .video()
            .expect("SDL video subsystem should be able to init");
        let window = video
            .window("title", 960, 540)
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

        Sdl2Pal {
            sdl_context,
            time,
            canvas: RefCell::new(canvas),
            exit_requested: Cell::new(false),
            texture_creator,
            textures: RefCell::new(Vec::new()),
            hids: RefCell::new(vec![Hid::Keyboard]),
            files: RefCell::new(Vec::new()),
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

    pub fn run_game_loop(&self, engine: &mut dyn EngineCallbacks) {
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

        while !self.exit_requested.get() {
            for event in event_pump.poll_iter() {
                match event {
                    Event::Quit { .. } => {
                        self.exit_requested.set(true);
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
                            pal::Event::DigitalInputPressed(
                                InputDevice::new(0),
                                button_for_scancode(scancode),
                            ),
                            Duration::from_millis(timestamp as u64),
                            self,
                        );
                    }

                    Event::KeyUp {
                        timestamp,
                        scancode: Some(scancode),
                        ..
                    } => {
                        engine.event(
                            pal::Event::DigitalInputReleased(
                                InputDevice::new(0),
                                button_for_scancode(scancode),
                            ),
                            Duration::from_millis(timestamp as u64),
                            self,
                        );
                    }

                    Event::ControllerButtonDown {
                        timestamp,
                        which,
                        button,
                    } => {
                        if let Some(device) = self.get_input_device_by_sdl_joystick_id(which) {
                            engine.event(
                                pal::Event::DigitalInputPressed(device, button_for_gamepad(button)),
                                Duration::from_millis(timestamp as u64),
                                self,
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
                                pal::Event::DigitalInputReleased(
                                    device,
                                    button_for_gamepad(button),
                                ),
                                Duration::from_millis(timestamp as u64),
                                self,
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

            engine.iterate(self);

            {
                let mut canvas = self.canvas.borrow_mut();
                canvas.present();
            }
        }
    }
}

impl Default for Sdl2Pal {
    fn default() -> Self {
        Self::new()
    }
}

impl Pal for Sdl2Pal {
    fn draw_area(&self) -> (f32, f32) {
        let (w, h) = {
            let canvas = self.canvas.borrow();
            canvas.viewport().size()
        };
        (w as f32, h as f32)
    }

    fn draw_triangles(&self, vertices: &[Vertex], indices: &[u32], settings: DrawSettings) {
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
                pal::BlendMode::None => SDL_BlendMode::SDL_BLENDMODE_NONE,
                pal::BlendMode::Blend => SDL_BlendMode::SDL_BLENDMODE_BLEND,
                pal::BlendMode::Add => SDL_BlendMode::SDL_BLENDMODE_ADD,
            };
            let scale_mode = match settings.texture_filter {
                pal::TextureFilter::NearestNeighbor => SDL_ScaleMode::SDL_ScaleModeNearest,
                pal::TextureFilter::Anisotropic => SDL_ScaleMode::SDL_ScaleModeBest,
            };
            let texture = if let Some(texture_index) = settings.texture {
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

    fn create_texture(
        &self,
        width: u16,
        height: u16,
        format: pal::PixelFormat,
    ) -> Option<pal::TextureRef> {
        let fmt = match format {
            // Unsure why ABGR8888 reads `[r, g, b, a, r, ...]` correctly, but here we are.
            pal::PixelFormat::Rgba => PixelFormatEnum::ABGR8888,
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
        Some(pal::TextureRef::new(texture_index as u64))
    }

    fn update_texture(
        &self,
        texture: platform_abstraction_layer::TextureRef,
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
                println!("[Sdl2Pal::update_texure]: texture update failed: {err}");
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
            });
            FileHandle::new(i)
        };
        Some(handle)
    }

    fn begin_file_read(
        &self,
        file: FileHandle,
        first_byte: u64,
        buffer: pal::Box<[u8]>,
    ) -> FileReadTask {
        // This is not an efficient implementation, it's a proof of concept.
        let id = {
            let mut files = self.files.borrow_mut();
            let file = files
                .get_mut(file.inner() as usize)
                .expect("invalid FileHandle");
            let id = file.tasks.len() as u64;
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

    fn finish_file_read(&self, task: FileReadTask) -> Result<pal::Box<[u8]>, pal::Box<[u8]>> {
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
                    println!("[Sdl2Pal::poll_file_read]: could not read file: {err}");
                    return Err(buffer);
                }
            }
        };
        Ok(written_buffer)
    }

    fn available_parallellism(&self) -> usize {
        thread::available_parallelism().unwrap().get()
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

    fn elapsed(&self) -> Duration {
        // Not using Instant even though we have std, to make timestamps between
        // SDL events and this consistent.
        Duration::from_millis(self.time.ticks64())
    }

    fn println(&self, message: Arguments) {
        println!("[Sdl2Pal::println]: {message}");
    }

    fn exit(&self, clean: bool) {
        if !clean {
            exit(1);
        }
        self.exit_requested.set(true);
    }

    fn malloc(&self, size: usize) -> *mut c_void {
        // Safety: ffi is unsafe by default, but there's nothing to ensure here.
        unsafe { SDL_malloc(size) }
    }

    unsafe fn free(&self, ptr: *mut c_void) {
        SDL_free(ptr)
    }
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
