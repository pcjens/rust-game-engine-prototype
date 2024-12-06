use pal::Pal;

use crate::{linear_allocator::Pool, LinearAllocator};

#[repr(C, align(64))]
pub struct Chunk([u8; 1_000_000]);

/// Resource manager for the engine.
///
/// Allocates memory in fixed-size chunks from a [Pool]. This wastes some memory
/// (the unused part of individual chunks), but since we're using a [Pool]
/// instead of a [LinearAllocator], individual resources can be dropped to free
/// up resources, instead of requiring resetting the whole allocator.
pub struct Resources<'platform, 'allocation> {
    platform: &'platform dyn Pal,
    pool: Pool<'allocation, Chunk>,
}

impl Resources<'_, '_> {
    pub fn new<'platform, 'allocation>(
        platform: &'platform dyn Pal,
        allocator: &'allocation LinearAllocator,
    ) -> Option<Resources<'platform, 'allocation>> {
        let pool = Pool::new(allocator)?;
        Some(Resources { platform, pool })
    }
}
