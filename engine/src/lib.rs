#![no_std]

#[cfg(test)]
mod test_platform;

mod arena;
mod engine;
mod event;
mod input;

pub use arena::{Arena, FixedVec};
pub use engine::Engine;
pub use event::Event;
pub use input::{Action, ActionKind, EventQueue, InputDeviceState, QueuedEvent};
