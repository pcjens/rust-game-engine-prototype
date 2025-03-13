// SPDX-FileCopyrightText: 2024 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use core::ops::Range;

use platform::{Platform, SpriteRef};

use crate::resources::{CHUNK_SIZE, SPRITE_CHUNK_DIMENSIONS, SPRITE_CHUNK_FORMAT};

/// Metadata for loading in a [`ChunkData`].
#[derive(Debug, Clone)]
pub struct ChunkDescriptor {
    /// The range of bytes in the chunk data portion of the database this
    /// sprite chunk can be loaded from.
    pub source_bytes: Range<u64>,
}

/// Metadata for loading in a [`SpriteChunkData`].
#[derive(Debug, Clone)]
pub struct SpriteChunkDescriptor {
    /// The width of the sprite the chunk contains.
    pub region_width: u16,
    /// The height of the sprite the chunk contains.
    pub region_height: u16,
    /// The range of bytes in the chunk data portion of the database this
    /// sprite chunk can be loaded from.
    pub source_bytes: Range<u64>,
}

/// Loaded memory for a single regular chunk. Contains [`CHUNK_SIZE`] bytes.
#[repr(C, align(64))]
pub struct ChunkData(pub [u8; CHUNK_SIZE as usize]);

impl ChunkData {
    /// Creates a zeroed-out [`ChunkData`].
    pub const fn empty() -> ChunkData {
        ChunkData([0; CHUNK_SIZE as usize])
    }

    /// Replaces the chunk contents with the given buffer, based on the
    /// [`ChunkDescriptor`] metadata.
    pub fn update(&mut self, descriptor: &ChunkDescriptor, buffer: &[u8]) {
        let len = (descriptor.source_bytes.end - descriptor.source_bytes.start) as usize;
        self.0[..len].copy_from_slice(buffer);
    }
}

/// Loaded (video) memory for a single sprite chunk. Contains a reference to a
/// loaded sprite, ready for drawing, with the size and format
/// [`SPRITE_CHUNK_DIMENSIONS`] and [`SPRITE_CHUNK_FORMAT`].
pub struct SpriteChunkData(pub SpriteRef);

impl SpriteChunkData {
    /// Creates a new sprite chunk from a newly created platform-dependent
    /// sprite.
    pub fn empty(platform: &dyn Platform) -> Option<SpriteChunkData> {
        let (w, h) = SPRITE_CHUNK_DIMENSIONS;
        let format = SPRITE_CHUNK_FORMAT;
        Some(SpriteChunkData(platform.create_sprite(w, h, format)?))
    }

    /// Uploads the pixel data from the buffer into the sprite, based on the
    /// [`SpriteChunkDescriptor`] metadata.
    pub fn update(
        &mut self,
        descriptor: &SpriteChunkDescriptor,
        buffer: &[u8],
        platform: &dyn Platform,
    ) {
        platform.update_sprite(
            self.0,
            0,
            0,
            descriptor.region_width,
            descriptor.region_height,
            buffer,
        );
    }
}
