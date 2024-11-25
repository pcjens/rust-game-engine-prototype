#![no_std]

/// Trait for using platform-dependent features from the engine without
/// depending on any platform directly. Implemented by the local pal-* crates.
pub trait Pal {
    /// Close the game.
    fn exit(&mut self, clean: bool) -> !;
}
