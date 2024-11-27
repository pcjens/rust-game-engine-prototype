#![no_std]

/// Trait for using platform-dependent features from the engine without
/// depending on any platform directly.
pub trait Pal {
    /// Exit the process, with `clean: false` if intending to signal failure.
    fn exit(&mut self, clean: bool) -> !;
}
