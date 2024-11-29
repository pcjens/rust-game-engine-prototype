#![no_std]

mod arena;

use core::time::Duration;

use pal::Pal;

pub use arena::{Arena, FixedVec};

/// How much memory is allocated for the frame allocator. Note that the unused
/// tail will not necessarily hog up physical memory, thanks to virtual memory,
/// so this is more of a sane maximum where we should start bailing.
///
/// Logic: pick a target refresh rate like 60Hz, and relatively decent RAM like
/// DDR4 at 3200MHz. That kind of RAM can transfer 25.6GB/s at best, so we get
/// `25.6GB/s / 60Hz`, which is how many bytes we could write in a single frame
/// at absolute max throughput. This happens to be about ~430MB, which is not
/// that much these days.
const FRAME_MEM: usize = 25_600_000_000 / 60;

pub struct Engine<P: Pal> {
    pub platform: P,
    pub frame_arena: Arena,
}

impl<P: Pal> Engine<P> {
    /// Creates a new instance of the engine.
    pub fn new(platform: P) -> Engine<P> {
        let frame_arena =
            Arena::new::<P>(FRAME_MEM).expect("should have enough memory for the frame allocator");
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
