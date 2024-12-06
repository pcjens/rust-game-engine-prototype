#![no_std]

#[cfg(test)]
mod test_platform;

mod engine;
mod event;
mod input;
mod linear_allocator;
mod resources;

pub use engine::Engine;
pub use event::Event;
pub use input::{Action, ActionKind, EventQueue, InputDeviceState, QueuedEvent};
pub use linear_allocator::{FixedVec, LinearAllocator, Pool, PoolBox};
pub use resources::Resources;
