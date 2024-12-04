#[cfg(feature = "platform-sdl2")]
fn main() {
    let platform = platform_sdl2::Sdl2Pal::new();
    let persistent_arena =
        engine::LinearAllocator::new(&platform, engine::Engine::PERSISTENT_MEMORY_SIZE)
            .expect("should have enough memory for the persistent arena");
    let engine = engine::Engine::new(&platform, &persistent_arena);
    platform_sdl2::run(engine, &platform);
}

#[cfg(not(any(feature = "platform-sdl2")))]
fn main() {
    compile_error!("at least one of the following platform features is required: 'platform-sdl2'");
}
