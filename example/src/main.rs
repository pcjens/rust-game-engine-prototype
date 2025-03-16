// SPDX-FileCopyrightText: 2024 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

#[cfg(feature = "sdl2")]
fn main() {
    use engine::{
        allocators::{static_allocator, LinearAllocator},
        Engine, EngineLimits,
    };
    use platform_sdl2::Sdl2Platform;

    #[cfg(feature = "profile")]
    profiling::tracy_client::Client::start();

    let platform = Sdl2Platform::new("example game");
    static PERSISTENT_ARENA: &LinearAllocator = static_allocator!(64 * 1024 * 1024);
    let mut engine = Engine::new(&platform, PERSISTENT_ARENA, EngineLimits::DEFAULT);
    platform.run_game_loop(&mut engine);
}

#[cfg(not(any(feature = "sdl2")))]
fn main() {
    compile_error!("at least one of the following platform features is required: 'sdl2'");
}
