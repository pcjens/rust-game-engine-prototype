#![no_std]

use core::ffi::c_void;

use bytemuck::{Pod, Zeroable};

/// "Platform abstraction layer": a trait for using platform-dependent features
/// from the engine without depending on any platform directly. All the
/// functions have a `&self` parameter, so that the methods can access some
/// (possibly internally mutable) state, but still keeping the platform object
/// as widely usable as possible (a "platform" is about as global an object as
/// you get). Also, none of these functions are (supposed to be) hot, and this
/// trait is object safe, so using &dyn [Pal] should be fine performance-wise,
/// and will hopefully help with compilation times by avoiding generics.
pub trait Pal {
    /// Get the current screen size. Could be physical pixels, could be
    /// "logical" pixels, depends on the platform, but it's the same coordinate
    /// system as the [Vertex]es passed into [Pal::draw_triangles].
    fn draw_area(&self) -> (f32, f32);

    /// Render out a pile of triangles.
    fn draw_triangles(&self, vertices: &[Vertex], indices: &[u32], settings: DrawSettings);

    /// Creates a texture from the given pixels, which can be used to draw
    /// triangles with it. The layout of `pixels` is RGBA. Returns None if the
    /// texture could not be created.
    fn create_texture(&self, width: u32, height: u32, pixels: &mut [u8]) -> Option<TextureRef>;

    /// Print out a string. For very crude debugging.
    fn println(&self, message: &str);

    /// Request the process to exit, with `clean: false` if intending to signal
    /// failure. On a clean exit, the exit may be delayed until a moment later,
    /// e.g. at the end of the current frame of the game loop, and after
    /// resource clean up. In failure cases, the idea is to bail asap, but it's
    /// up to the platform.
    fn exit(&self, clean: bool);

    /// Allocate the given amount of bytes (returning a null pointer on error).
    /// Not called often from the engine, memory is allocated in big chunks, so
    /// this can be slow and defensively implemented.
    fn malloc(&self, size: usize) -> *mut c_void;

    /// Free the memory allocated by [Pal::malloc]. Not called often from the
    /// engine, memory is allocated in big chunks, so this can be slow and
    /// defensively implemented.
    ///
    /// ## Safety
    ///
    /// - Since the implementation is free to free the memory, the memory
    ///   pointed at by the given pointer shouldn't be accessed after calling
    ///   this.
    /// - The `size` parameter must be the same value passed into the matching
    ///   `malloc` call.
    unsafe fn free(&self, ptr: *mut c_void, size: usize);
}

/// Vertex describing a 2D point with a texture coordinate and a color.
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
    /// Creates a [Vertex] with zeroed texture coordinates and a white color,
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

    /// Creates a [Vertex] with the given position and texture coordinates, and
    /// no color modulation (white vertex colors).
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

/// Platform-specific texture reference. No guarantees about the texture
/// actually existing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextureRef(u64);
impl TextureRef {
    /// Creates a new [TextureRef]. Should only be created in the [Pal]
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
