#[cfg(feature = "platform-sdl2")]
fn main() {
    let platform = platform_sdl2::Sdl2Pal::new();
    static PERSISTENT_ARENA: &engine::allocators::StaticAllocator =
        engine::allocators::static_allocator!(1_000_000);
    let mut engine = engine::Engine::new(&platform, PERSISTENT_ARENA);
    platform.run_game_loop(&mut engine);
}

#[cfg(not(any(feature = "platform-sdl2")))]
fn main() {
    compile_error!("at least one of the following platform features is required: 'platform-sdl2'");
}
