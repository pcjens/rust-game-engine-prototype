// SPDX-FileCopyrightText: 2024 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Asset type for individual images that can be rendered as-is.
//!
//! Not really suitable for use as a texture atlas, due to color bleeding
//! issues.

use core::ops::Range;

use arrayvec::ArrayVec;

use crate::{
    geom::Rect,
    renderer::{self, texture::RenderableTexture, DrawQueue},
    resources::{ResourceDatabase, ResourceLoader},
};

use super::{gen_asset_handle_code, Asset};

gen_asset_handle_code!(
    TextureAsset,
    TextureHandle,
    find_texture,
    get_texture,
    textures
);

/// The maximum amount of mip levels for a texture.
pub const MAX_MIPS: usize = 12;

/// One mipmap level (i.e. a texture with a specific resolution) of a
/// [`TextureAsset`].
///
/// Each [`TextureAsset`] has a maximum of [`MAX_MIPS`] of these, with the first
/// level being the resolution of the original texture, and each successive mip
/// having half the width and height of the previous mip.
///
/// The engine does not use hardware mipmapping, to keep the platform
/// abstraction simpler. Multiple mip levels (especially the smaller ones) can
/// share a single texture chunk.
#[derive(Debug)]
pub enum TextureMipLevel {
    /// A texture contained within a single chunk. Unlike the other variant,
    /// these textures might not be located in the top-left corner of the chunk,
    /// so this variant has an offset field.
    SingleChunkTexture {
        /// Offset from the topmost and leftmost chunks where the actual texture
        /// starts.
        offset: (u16, u16),
        /// The dimensions of the texture in pixels.
        size: (u16, u16),
        /// The chunk the texture's pixels are located in. The subregion to
        /// render is described by the `offset` and `size` fields.
        texture_chunk: u32,
    },
    /// A texture split between multiple texture chunks.
    ///
    /// Chunks are allocated for a multi-chunk texture starting from the
    /// top-left, row by row. Each chunk has a 1px border (copied from the edge
    /// of the texture, creating a kind of clamp-to-edge effect), inside which
    /// is the actual texture. The chunks on the right and bottom edges of the
    /// texture are the only ones that don't occupy their texture chunk
    /// entirely, they instead occupy only up to the texture's `width` and
    /// `height` plus the border, effectively taking up a `width + 2` by `height
    /// + 2` region from the top left corner of those chunks due to the border.
    MultiChunkTexture {
        /// The dimensions of the texture in pixels.
        size: (u16, u16),
        /// The chunks the texture is made up of.
        texture_chunks: Range<u32>,
    },
}

/// Drawable image.
#[derive(Debug)]
pub struct TextureAsset {
    /// Whether the texture's alpha should be taken into consideration while
    /// rendering.
    pub transparent: bool,
    /// The actual specific-size textures used for rendering depending on the
    /// size of the texture on screen.
    pub mip_chain: ArrayVec<TextureMipLevel, MAX_MIPS>,
}

impl Asset for TextureAsset {
    fn get_chunks(&self) -> Option<Range<u32>> {
        None
    }

    fn offset_chunks(&mut self, _offset: i32) {}

    fn get_texture_chunks(&self) -> Option<Range<u32>> {
        let mut range: Option<Range<u32>> = None;
        for mip in &self.mip_chain {
            let mip_range = match mip {
                TextureMipLevel::SingleChunkTexture { texture_chunk, .. } => {
                    *texture_chunk..*texture_chunk + 1
                }
                TextureMipLevel::MultiChunkTexture { texture_chunks, .. } => texture_chunks.clone(),
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

    fn offset_texture_chunks(&mut self, offset: i32) {
        for mip in &mut self.mip_chain {
            match mip {
                TextureMipLevel::SingleChunkTexture { texture_chunk, .. } => {
                    *texture_chunk = (*texture_chunk as i32 + offset) as u32;
                }
                TextureMipLevel::MultiChunkTexture { texture_chunks, .. } => {
                    texture_chunks.start = (texture_chunks.start as i32 + offset) as u32;
                    texture_chunks.end = (texture_chunks.end as i32 + offset) as u32;
                }
            };
        }
    }
}

impl TextureAsset {
    /// Draw this texture into the `dst` rectangle.
    ///
    /// Returns false if the texture couldn't be drawn due to the draw queue
    /// filling up.
    #[must_use]
    pub fn draw(
        &self,
        dst: Rect,
        draw_order: u8,
        draw_queue: &mut DrawQueue,
        resources: &ResourceDatabase,
        resource_loader: &mut ResourceLoader,
    ) -> bool {
        renderer::texture::draw(
            RenderableTexture {
                mip_chain: &self.mip_chain,
                transparent: self.transparent,
                draw_order,
            },
            dst,
            draw_queue,
            resources,
            resource_loader,
        )
    }
}
