#[cfg(feature = "platform-sdl2")]
fn main() {
    let platform = platform_sdl2::Sdl2Pal::new();
    let persistent_arena = engine::allocators::LinearAllocator::new(&platform, 100_000)
        .expect("persistent game engine memory allocation should not fail");
    let mut engine = engine::Engine::new(&platform, &persistent_arena);
    platform.run_game_loop(&mut engine);
}

#[cfg(not(any(feature = "platform-sdl2")))]
fn main() {
    compile_error!("at least one of the following platform features is required: 'platform-sdl2'");
}
