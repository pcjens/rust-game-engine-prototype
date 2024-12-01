use std::{process::exit, time::Duration};

use engine::Engine;
use pal::Pal;
use sdl2::{event::Event, pixels::Color};
use sdl2_sys::{SDL_free, SDL_malloc};

/// The [Pal] impl for the SDL2 based platform.
pub struct Sdl2Pal {
    sdl_context: sdl2::Sdl,
}

impl Sdl2Pal {
    pub fn new() -> Sdl2Pal {
        let sdl_context = sdl2::init().expect("SDL 2 library should be able to init");
        Sdl2Pal { sdl_context }
    }
}

impl Pal for Sdl2Pal {
    fn println(&self, message: &str) {
        println!("{message}");
    }

    fn exit(&self, clean: bool) -> ! {
        exit(if clean { 0 } else { 1 });
    }

    fn malloc(&self, size: usize) -> *mut std::ffi::c_void {
        // Safety: ffi is unsafe by default, but there's nothing to ensure here.
        unsafe { SDL_malloc(size) }
    }

    unsafe fn free(&self, ptr: *mut std::ffi::c_void, _size: usize) {
        SDL_free(ptr)
    }
}

pub fn run(mut engine: Engine, platform: &Sdl2Pal) -> ! {
    let time = platform
        .sdl_context
        .timer()
        .expect("SDL timer subsystem should be able to init");

    let video = platform
        .sdl_context
        .video()
        .expect("SDL video subsystem should be able to init");
    let window = video
        .window("title", 960, 540)
        .allow_highdpi()
        .position_centered()
        .resizable()
        .build()
        .expect("should be able to create a window");
    let mut canvas = window
        .into_canvas()
        .present_vsync()
        .build()
        .expect("should be able to create a renderer");

    let mut event_pump = platform
        .sdl_context
        .event_pump()
        .expect("SDL 2 event pump should init without issue");

    loop {
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. } => {
                    drop(engine);
                    exit(0);
                }
                _ => {}
            }
        }

        canvas.set_draw_color(Color::BLACK);
        canvas.clear();

        engine.iterate(Duration::from_millis(time.ticks64()));

        canvas.present();
    }
}
