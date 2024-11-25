#![no_std]

use pal::Pal;

pub struct Engine<P: Pal> {
    pub platform: P,
}

impl<P: Pal> Engine<P> {
    /// [For [pal] implementers.] Creates a new instance of the engine.
    pub fn new(platform: P) -> Engine<P> {
        Engine { platform }
    }

    /// [For [pal] implementers.] Runs one iteration of the game loop.
    pub fn iterate(&mut self) {}
}
