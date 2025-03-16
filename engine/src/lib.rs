// SPDX-FileCopyrightText: 2024 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! This crate makes up most of the runtime engine, containing all engine
//! systems except for the platform-specific ones. The platform-specifics are
//! implemented by platform crates, which call and are called from this crate
//! via the [`platform`] traits.
//!
//! Since game engines consist of relatively independent systems, most of this
//! crate is not exported in the root, but instead in the top-level modules,
//! each exposing a distinct part of the engine.
//!
//! Generic core systems and types are provided by:
//! - [`allocators`]: Memory allocators used by other parts of the engine,
//!       mostly collections, for allocating dynamic amounts of data. Allocators
//!       can be allocated from other allocators, and as such, the engine takes
//!       one main allocator in [`Engine::new`] which is used directly or
//!       indirectly for all allocations made by the engine.
//! - [`collections`]: Simple collection types oriented around up-front
//!       allocation. There are no Vec-style reallocating collections, to make
//!       using linear allocators feasible, which in turn is desirable for
//!       performance.
//! - [`geom`]: Geometry types and related math operations.
//! - [`multithreading`]: Utilities for spreading work between multiple CPU
//!       cores.
//!
//! Specific game engine systems can be found in:
//! - [`resources`]: Resource/game asset types and their loading systems.
//! - [`renderer`]: Low-level renderer.
//! - [`input`]: Input handling.
//! - [`mixer`]: Audio playback.
//! - [`game_objects`]: A scene/game object/component system to build gameplay
//!       systems on.

#![no_std]
#![warn(missing_docs)]

// Exported to allow instrumenting functions generated with macros.
pub use profiling;

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
/// Runtime game object model types and functionality.
pub mod game_objects;
/// Geometry related types and operations.
pub mod geom;
/// Input events and their translation into game-specific actions.
pub mod input;
/// Audio playback system and types.
pub mod mixer;
/// Utilities for splitting work to be processed in parallel.
pub mod multithreading;
/// Low-level graphics-related data structures and functionality.
pub mod renderer;
/// The resource database and everything related to querying, loading, and using
/// assets from it.
pub mod resources;

mod engine;

pub use engine::{Engine, EngineLimits, Game};
