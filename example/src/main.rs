#[cfg(feature = "platform-sdl2")]
fn main() {
    let platform = platform_sdl2::Sdl2Pal::new();
    let engine = engine::Engine::new(platform);
    platform_sdl2::run(engine);
}

#[cfg(not(any(feature = "platform-sdl2")))]
fn main() {
    compile_error!("at least one of the following platform features is required: 'platform-sdl2'");
}
