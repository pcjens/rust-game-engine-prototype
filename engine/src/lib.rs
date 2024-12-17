//! ## Codebase-specific conventions
//!
//! - The `'eng` lifetime: to make it easier to recognize "engine-static"
//!   lifetimes (e.g. borrows that can live for the whole game), lifetimes are
//!   named this if they are bound to the same lifetime as the [`Engine`] type's
//!   lifetime parameter.
//! - Another convention-based lifetime name is `'frm`, which refers to the
//!   lifetime of the frame arena, which is scoped to one iteration of the game
//!   loop.

#![no_std]

#[cfg(test)]
mod test_platform;

mod engine;
mod input;
mod linear_allocator;
pub mod resources;

pub use engine::Engine;
pub use input::{Action, ActionKind, EventQueue, InputDeviceState, QueuedEvent};
pub use linear_allocator::{FixedVec, LinearAllocator, Pool, PoolBox};
pub use resources::Resources;
