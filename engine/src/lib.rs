#![no_std]

mod arena;
#[cfg(test)]
mod test_platform;

use core::time::Duration;

use pal::Pal;

pub use arena::{Arena, FixedVec};

/// How much memory is allocated for the frame allocator.
const FRAME_MEM: usize = 1_000_000_000;

pub struct Engine<'platform> {
    pub platform: &'platform dyn Pal,
    pub frame_arena: Arena<'platform>,
}

impl Engine<'_> {
    /// Creates a new instance of the engine.
    pub fn new(platform: &dyn Pal) -> Engine {
        let frame_arena = Arena::new(platform, FRAME_MEM)
            .expect("should have enough memory for the frame allocator");
        Engine {
            platform,
            frame_arena,
        }
    }

    /// Runs one iteration of the game loop.
    pub fn iterate(&mut self, _time_since_start: Duration) {
        self.frame_arena.reset();
    }
}
