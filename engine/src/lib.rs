#![no_std]

mod arena;

use core::time::Duration;

use pal::Pal;

pub struct Engine<P: Pal> {
    pub platform: P,
}

impl<P: Pal> Engine<P> {
    /// Creates a new instance of the engine.
    pub fn new(platform: P) -> Engine<P> {
        Engine { platform }
    }

    /// Runs one iteration of the game loop.
    pub fn iterate(&mut self, _time_since_start: Duration) {}
}
