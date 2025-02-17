//! TODO: write a outline of the engine here

#![no_std]
#![warn(missing_docs)]

#[cfg(any(test, doctest))]
/// A simple platform implementation for use in tests.
pub mod test_platform;

/// Low-level memory allocators used for all dynamic allocation in the engine.
///
/// The idea is to use any system allocators a few times at startup to create
/// these allocators, and then suballocate from that. This should keep
/// performance characteristics more predictable between different platforms.
pub mod allocators;
/// Collection types for varying memory access patterns. Backing memory provided
/// by allocators in the [allocators] module.
pub mod collections;
/// Geometry related types and operations.
pub mod geom;
/// Input events and their translation into game-specific actions.
pub mod input;
/// Utilities for splitting work to be processed in parallel.
pub mod multithreading;
/// Low-level graphics-related data structures and functionality.
pub mod renderer;
/// The resource database and everything related to querying, loading, and using
/// assets from it.
pub mod resources;

mod engine;

pub use engine::Engine;
