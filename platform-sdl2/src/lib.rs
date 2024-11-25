use std::process::exit;

use pal::Pal;
use sdl2::{event::Event, pixels::Color};

#[macro_export]
macro_rules! generate_main {
    () => {
        // Nothing odd in this main implentation, but e.g. a wasm entrypoint
        // would be more interesting, hence this generate_main! business.
        fn main() {
            platform_sdl2::main_impl();
        }
    };
}

#[doc(hidden)]
pub fn main_impl() {
    let sdl_context = sdl2::init().expect("SDL 2 library should be able to init");

    let mut engine = engine::Engine::new(Sdl2Pal { sdl_context });

    let video = engine
        .platform
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

    let mut event_pump = engine
        .platform
        .sdl_context
        .event_pump()
        .expect("SDL 2 event pump should init without issue");

    loop {
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. } => {
                    exit(0);
                }
                _ => {}
            }
        }

        canvas.set_draw_color(Color::BLACK);
        canvas.clear();

        engine.iterate();

        canvas.present();
    }
}

pub struct Sdl2Pal {
    sdl_context: sdl2::Sdl,
}

impl Pal for Sdl2Pal {
    fn exit(&mut self, clean: bool) -> ! {
        exit(if clean { 0 } else { 1 });
    }
}
