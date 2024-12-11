use std::{
    cell::{Cell, RefCell},
    ffi::{c_int, c_void},
    process::exit,
    ptr::{addr_of, null_mut},
    time::Duration,
};

use engine::Engine;
use platform_abstraction_layer::{
    self as pal, ActionCategory, Button, DrawSettings, InputDevice, InputDevices, Pal, Vertex,
};
use sdl2::{
    controller::Button as SdlButton,
    event::Event,
    keyboard::{Keycode, Mod, Scancode},
    pixels::{Color, PixelFormatEnum},
    rect::Rect,
    render::{Texture, TextureCreator, WindowCanvas},
    surface::Surface,
    video::WindowContext,
    Sdl,
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

/// The [Pal] impl for the SDL2 based platform.
pub struct Sdl2Pal {
    sdl_context: Sdl,
    canvas: RefCell<WindowCanvas>,
    exit_requested: Cell<bool>,
    texture_creator: &'static TextureCreator<WindowContext>,
    textures: RefCell<Vec<Texture<'static>>>,
    /// List of input devices. Devices are never removed, so the InputDevice ids
    /// used for this platform are indices to this list.
    hids: RefCell<Vec<Hid>>,
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

        let texture_creator = Box::leak(Box::new(canvas.texture_creator()));

        Sdl2Pal {
            sdl_context,
            canvas: RefCell::new(canvas),
            exit_requested: Cell::new(false),
            texture_creator,
            textures: RefCell::new(Vec::new()),
            hids: RefCell::new(vec![Hid::Keyboard]),
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
        width: u32,
        height: u32,
        pixels: &mut [u8],
    ) -> Option<pal::TextureRef> {
        // Unsure why ABGR8888 reads `[r, g, b, a, r, g, b, a]` correctly, but here we are.
        let fmt = PixelFormatEnum::ABGR8888;
        let surface = Surface::from_data(pixels, width, height, width * 4, fmt).ok()?;
        let texture = self
            .texture_creator
            .create_texture_from_surface(surface)
            .ok()?;
        let texture_index = {
            let mut textures = self.textures.borrow_mut();
            let idx = textures.len();
            textures.push(texture);
            idx
        };
        Some(pal::TextureRef::new(texture_index as u64))
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

    fn println(&self, message: &str) {
        println!("{message}");
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

    unsafe fn free(&self, ptr: *mut c_void, _size: usize) {
        SDL_free(ptr)
    }
}

pub fn run(mut engine: Engine, platform: &Sdl2Pal) {
    let time = platform
        .sdl_context
        .timer()
        .expect("SDL timer subsystem should be able to init");
    // Init the subsystem. The subsystem is actually used, just through the FFI
    // calls, since the subsystem doesn't expose everything we need (e.g. game
    // controller type).
    let _gamepad = platform
        .sdl_context
        .game_controller()
        .expect("SDL controller subsystem should be able to init");
    let mut event_pump = platform
        .sdl_context
        .event_pump()
        .expect("SDL 2 event pump should init without issue");

    while !platform.exit_requested.get() {
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. } => {
                    platform.exit_requested.set(true);
                }
                Event::KeyDown {
                    keycode: Some(Keycode::Q),
                    keymod,
                    ..
                } if keymod.intersects(Mod::LCTRLMOD) => {
                    platform.exit_requested.set(true);
                }

                Event::ControllerDeviceAdded { which, .. } => {
                    // Safety: ffi call.
                    let controller = unsafe { SDL_GameControllerOpen(which as i32) };
                    if !controller.is_null() {
                        let mut hids = platform.hids.borrow_mut();
                        hids.push(Hid::Gamepad {
                            controller,
                            connected: true,
                            instance_id: which,
                        });
                    }
                }
                Event::ControllerDeviceRemoved { which, .. } => {
                    let mut hids = platform.hids.borrow_mut();
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
                        engine::Event::DigitalInputPressed(
                            InputDevice::new(0),
                            button_for_scancode(scancode),
                        ),
                        Duration::from_millis(timestamp as u64),
                    );
                }

                Event::KeyUp {
                    timestamp,
                    scancode: Some(scancode),
                    ..
                } => {
                    engine.event(
                        engine::Event::DigitalInputReleased(
                            InputDevice::new(0),
                            button_for_scancode(scancode),
                        ),
                        Duration::from_millis(timestamp as u64),
                    );
                }

                Event::ControllerButtonDown {
                    timestamp,
                    which,
                    button,
                } => {
                    if let Some(device) = platform.get_input_device_by_sdl_joystick_id(which) {
                        engine.event(
                            engine::Event::DigitalInputPressed(device, button_for_gamepad(button)),
                            Duration::from_millis(timestamp as u64),
                        );
                    }
                }

                Event::ControllerButtonUp {
                    timestamp,
                    which,
                    button,
                } => {
                    if let Some(device) = platform.get_input_device_by_sdl_joystick_id(which) {
                        engine.event(
                            engine::Event::DigitalInputReleased(device, button_for_gamepad(button)),
                            Duration::from_millis(timestamp as u64),
                        );
                    }
                }

                _ => {}
            }
        }

        {
            let mut canvas = platform.canvas.borrow_mut();
            canvas.set_draw_color(Color::BLACK);
            canvas.clear();
        }

        engine.iterate(Duration::from_millis(time.ticks64()));

        {
            let mut canvas = platform.canvas.borrow_mut();
            canvas.present();
        }
    }
}

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
