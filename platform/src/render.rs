// SPDX-FileCopyrightText: 2024 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use bytemuck::{Pod, Zeroable};

/// Vertex describing a 2D point with a texture coordinate and a color.
#[derive(Debug, Default, Clone, Copy)]
#[repr(C)]
pub struct Vertex {
    /// The horizontal coordinate of the position of this vertex.
    pub x: f32,
    /// The vertical coordinate of the position of this vertex.
    pub y: f32,

    /// The horizontal texture coordinate of this vertex, 0 referring to the
    /// left edge, and 1 referring to the right edge of the texture.
    pub u: f32,
    /// The vertical texture coordinate of this vertex, 0 referring to the top
    /// edge, and 1 referring to the bottom edge of the texture.
    pub v: f32,

    /// The red channel of the vertex color, which is multiplied with the
    /// texture's red channel (or used as is, if no texture is used).
    pub r: u8,
    /// The green channel of the vertex color, which is multiplied with the
    /// texture's green channel (or used as is, if no texture is used).
    pub g: u8,
    /// The blue channel of the vertex color, which is multiplied with the
    /// texture's blue channel (or used as is, if no texture is used).
    pub b: u8,
    /// The alpha channel of the vertex color, which is multiplied with the
    /// texture's alpha channel (or used as is, if no texture is used).
    pub a: u8,
}

// Safety: Vertex is "inhabited" and all zeroes is a valid value for it, all the
// fields are Zeroable.
unsafe impl Zeroable for Vertex {}

// Safety: manually checked for f32 typed x/y/u/v at the beginning with u8 typed
// r/g/b/a at the end.
// - The type must be inhabited: it is.
// - The type must allow any bit pattern: it does, f32 and u8 are Pod.
// - The type must not contain any uninit (or padding) bytes: it does not, it's
//   4-aligned and the first four fields are 4 bytes, and the last four are
//   1-byte aligned and there's 4 of them.
// - The type needs to have all fields also be `Pod`: it does.
// - The type needs to be `repr(C)` or...: yes, it is repr(C) and does not
//   specify padding or alignment manually.
// - It is disallowed for types to contain pointer types, `Cell`, `UnsafeCell`,
//   atomics, and any other forms of interior mutability: none of those in this.
// - More precisely: A shared reference to the type must allow reads, and *only*
//   reads: yes, no hidden inner mutability tricks.
unsafe impl Pod for Vertex {}

impl Vertex {
    /// Creates a [`Vertex`] with zeroed texture coordinates and a white color,
    /// with the given coordinates.
    pub fn xy(x: f32, y: f32) -> Vertex {
        Vertex {
            x,
            y,
            u: 0.0,
            v: 0.0,
            r: 0xFF,
            g: 0xFF,
            b: 0xFF,
            a: 0xFF,
        }
    }

    /// Creates a [`Vertex`] with the given position and texture coordinates,
    /// and no color modulation (white vertex colors).
    pub fn new(x: f32, y: f32, u: f32, v: f32) -> Vertex {
        Vertex {
            x,
            y,
            u,
            v,
            r: 0xFF,
            g: 0xFF,
            b: 0xFF,
            a: 0xFF,
        }
    }
}

/// Various options for controlling how draw commands should be executed.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct DrawSettings {
    /// If None, the vertex colors are used to draw solid triangles.
    pub texture: Option<TextureRef>,
    /// The blending mode of the draw, i.e. how the pixels are mixed from this
    /// draw and any previous draws in the same area.
    pub blend_mode: BlendMode,
    /// The filtering mode used to stretch or squeeze the texture when the
    /// rendering resolution doesn't exactly match the texture.
    pub texture_filter: TextureFilter,
    /// The draw will only apply to pixels within this rectangle. Layout: `[x,
    /// y, width, height]`.
    pub clip_area: Option<[f32; 4]>,
}

/// Platform-specific texture reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextureRef(u64);

impl TextureRef {
    /// Creates a new [`TextureRef`]. Should only be created in the platform
    /// implementation, which also knows how the inner value is going to be
    /// used.
    pub fn new(id: u64) -> TextureRef {
        TextureRef(id)
    }

    /// Returns the inner value passed into [`TextureRef::new`]. Generally only
    /// relevant to the platform implementation.
    pub fn inner(self) -> u64 {
        self.0
    }
}

/// How drawn pixels are blended with the previously drawn pixels.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BlendMode {
    /// All channels are replaced with the color being drawn, including alpha.
    None,
    /// `dstRGB = (srcRGB * srcA) + (dstRGB * (1 - srcA))`  
    /// `dstA = srcA + (dstA * (1 - srcA))`
    ///
    /// Where `dst` is the color of the framebuffer, and `src` is the color
    /// being drawn on it.
    #[default]
    Blend,
    /// `dstRGB = (srcRGB * srcA) + dstRGB`  
    /// `dstA = dstA`
    ///
    /// Where `dst` is the color of the framebuffer, and `src` is the color
    /// being drawn on it.
    Add,
}

/// How the texture is filtered when magnified or minified.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TextureFilter {
    /// No blending, just picks the pixel from the texture which is nearest to
    /// the sampled position. When rendering at a higher resolution than the
    /// texture, this keeps pixel art sharp.
    NearestNeighbor,
    /// Linear blending between pixels, which blurs the texture when rendering
    /// at a higher resolution than the texture itself, but avoids noisy
    /// aliasing artifacts caused by [`TextureFilter::NearestNeighbor`].
    #[default]
    Linear,
}

/// Descriptions of pixel data layouts, used to interpret the byte arrays passed
/// into uploading functions.
#[derive(Debug)]
#[repr(u8)]
pub enum PixelFormat {
    /// 8-bit per channel RGBA colors, arranged in order: `[red, green, blue,
    /// alpha, red, ...]`.
    Rgba,
}

impl PixelFormat {
    /// Returns the amount of bytes each pixel takes up in a pixel buffer if
    /// that buffer is using this pixel format.
    ///
    /// E.g. for 8-bit RGBA, this returns 4, as each of the four channels takes
    /// up eight bits.
    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            PixelFormat::Rgba => 4,
        }
    }
}
