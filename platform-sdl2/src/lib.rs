use std::{
    cell::{Cell, RefCell},
    ffi::{c_int, c_void},
    process::exit,
    ptr::{addr_of, null_mut},
    time::Duration,
};

use engine::Engine;
use pal::Pal;
use sdl2::{
    event::Event,
    keyboard::{Keycode, Mod},
    pixels::{Color, PixelFormatEnum},
    rect::Rect,
    render::{Texture, TextureCreator, WindowCanvas},
    surface::Surface,
    video::WindowContext,
    Sdl,
};
use sdl2_sys::{
    SDL_BlendMode, SDL_Color, SDL_RenderGeometryRaw, SDL_Renderer, SDL_ScaleMode,
    SDL_SetTextureBlendMode, SDL_SetTextureScaleMode, SDL_free, SDL_malloc,
};

/// The [Pal] impl for the SDL2 based platform.
pub struct Sdl2Pal {
    sdl_context: Sdl,
    canvas: RefCell<WindowCanvas>,
    exit_requested: Cell<bool>,
    texture_creator: &'static TextureCreator<WindowContext>,
    textures: RefCell<Vec<Texture<'static>>>,
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

    fn draw_triangles(
        &self,
        vertices: &[pal::Vertex],
        indices: &[u32],
        settings: pal::DrawSettings,
    ) {
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
