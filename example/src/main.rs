// SPDX-FileCopyrightText: 2024 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

#[cfg(feature = "sdl2")]
fn main() {
    let platform = platform_sdl2::Sdl2Platform::new("example game");
    static PERSISTENT_ARENA: &engine::allocators::LinearAllocator =
        engine::allocators::static_allocator!(64 * 1024 * 1024);
    let mut engine = engine::Engine::new(&platform, PERSISTENT_ARENA, 8192);
    platform.run_game_loop(&mut engine);
}

#[cfg(not(any(feature = "sdl2")))]
fn main() {
    compile_error!("at least one of the following platform features is required: 'sdl2'");
}
