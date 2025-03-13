// SPDX-FileCopyrightText: 2024 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Asset type for individual images that can be rendered as-is.
//!
//! Not really suitable for use as a sprite atlas, due to color bleeding issues.

use core::ops::Range;

use arrayvec::ArrayVec;

use super::{gen_asset_handle_code, Asset};

gen_asset_handle_code!(SpriteAsset, SpriteHandle, find_sprite, get_sprite, sprites);

/// The maximum amount of mip levels for a sprite.
pub const MAX_MIPS: usize = 12;

/// One mipmap level (i.e. a sprite with a specific resolution) of a
/// [`SpriteAsset`].
///
/// Each [`SpriteAsset`] has a maximum of [`MAX_MIPS`] of these, with the first
/// level being the resolution of the original sprite, and each successive mip
/// having half the width and height of the previous mip.
///
/// The sprites do not use hardware mipmapping. Multiple mip levels (especially
/// the smaller ones) can share a single sprite chunk.
#[derive(Debug)]
pub enum SpriteMipLevel {
    /// A sprite contained within a single chunk. Unlike the other variant,
    /// these sprite might not be located in the top-left corner of the chunk,
    /// so this variant has an offset field.
    SingleChunkSprite {
        /// Offset from the topmost and leftmost chunks where the actual sprite
        /// starts.
        offset: (u16, u16),
        /// The dimensions of the sprite in pixels.
        size: (u16, u16),
        /// The chunk the sprite's pixels are located in. The subregion to
        /// render is described by the `offset` and `size` fields.
        sprite_chunk: u32,
    },
    /// A sprite split between multiple sprite chunks.
    ///
    /// Chunks are allocated for a multi-chunk sprite starting from the
    /// top-left, row by row. Each chunk has a 1px border (copied from the edge
    /// of the sprite, creating a kind of clamp-to-edge effect), inside which is
    /// the actual sprite. The chunks on the right and bottom edges of the
    /// sprite are the only ones that don't occupy their sprite chunk entirely,
    /// they instead occupy only up to the sprite's `width` and `height` plus
    /// the border, effectively taking up a `width + 2` by `height
    /// + 2` region from the top left corner of those chunks due to the border.
    MultiChunkSprite {
        /// The dimensions of the sprite in pixels.
        size: (u16, u16),
        /// The chunks the sprite is made up of.
        sprite_chunks: Range<u32>,
    },
}

/// Drawable image.
#[derive(Debug)]
pub struct SpriteAsset {
    /// Whether the sprite's alpha should be taken into consideration while
    /// rendering.
    pub transparent: bool,
    /// The actual specific-size sprites used for rendering depending on the
    /// size of the sprite on screen.
    pub mip_chain: ArrayVec<SpriteMipLevel, MAX_MIPS>,
}

impl Asset for SpriteAsset {
    fn get_chunks(&self) -> Option<Range<u32>> {
        None
    }

    fn offset_chunks(&mut self, _offset: i32) {}

    fn get_sprite_chunks(&self) -> Option<Range<u32>> {
        let mut range: Option<Range<u32>> = None;
        for mip in &self.mip_chain {
            let mip_range = match mip {
                SpriteMipLevel::SingleChunkSprite { sprite_chunk, .. } => {
                    *sprite_chunk..*sprite_chunk + 1
                }
                SpriteMipLevel::MultiChunkSprite { sprite_chunks, .. } => sprite_chunks.clone(),
            };
            if let Some(range) = &mut range {
                range.start = range.start.min(mip_range.start);
                range.end = range.end.max(mip_range.end);
            } else {
                range = Some(mip_range);
            }
        }
        range
    }

    fn offset_sprite_chunks(&mut self, offset: i32) {
        for mip in &mut self.mip_chain {
            match mip {
                SpriteMipLevel::SingleChunkSprite { sprite_chunk, .. } => {
                    *sprite_chunk = (*sprite_chunk as i32 + offset) as u32;
                }
                SpriteMipLevel::MultiChunkSprite { sprite_chunks, .. } => {
                    sprite_chunks.start = (sprite_chunks.start as i32 + offset) as u32;
                    sprite_chunks.end = (sprite_chunks.end as i32 + offset) as u32;
                }
            };
        }
    }
}
