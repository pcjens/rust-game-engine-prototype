use bytemuck::{Pod, Zeroable};

/// Vertex describing a 2D point with a texture coordinate and a color.
///
/// Texture coordinates (u, v) should be interpreted as "0, 0" referring to the
/// top-left corner of the texture and "1, 1" referring to the bottom-right
/// corner, with "repeat" tiling for coordinates outside of that range.
#[derive(Debug, Default, Clone, Copy, Zeroable, Pod)]
#[repr(C)]
pub struct Vertex {
    pub x: f32,
    pub y: f32,

    pub u: f32,
    pub v: f32,

    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

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
    pub blend_mode: BlendMode,
    pub texture_filter: TextureFilter,
    /// The draw will only apply to pixels within this rectangle. Layout: `[x,
    /// y, width, height]`.
    pub clip_area: Option<[f32; 4]>,
}

/// Platform-specific texture reference.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextureRef(u64);

impl TextureRef {
    /// Creates a new [`TextureRef`]. Should only be created in the platform
    /// implementation, which also knows how the inner value is going to be
    /// used.
    pub fn new(id: u64) -> TextureRef {
        TextureRef(id)
    }

    pub fn inner(self) -> u64 {
        self.0
    }
}

/// How drawn pixels are blended with the previously drawn pixels.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
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
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureFilter {
    NearestNeighbor,
    #[default]
    Anisotropic,
}

/// Descriptions of pixel data layouts, used to interpret the byte arrays passed
/// into uploading functions.
#[derive(Debug)]
pub enum PixelFormat {
    /// 8-bit per channel RGBA colors, arranged in order: `[red, green, blue,
    /// alpha, red, ...]`.
    Rgba,
}

impl PixelFormat {
    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            PixelFormat::Rgba => 4,
        }
    }
}
