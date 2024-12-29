pub mod asset_index;
pub mod assets;
pub mod chunks;
mod deserialize;
mod serialize;

use platform_abstraction_layer::PixelFormat;

pub use deserialize::{deserialize, Deserialize};
pub use serialize::{serialize, Serialize};

#[allow(unused_imports)] // used in docs
use asset_index::AssetIndex;

/// Magic number used when de/serializing [`AssetIndex`], to allow easily
/// marking resource databases as incompatible if breaking changes to the format
/// are made.
pub const RESOURCE_DB_MAGIC_NUMBER: u32 = 0xE97E6D00;
/// Amount of bytes in the regular dynamically allocated chunks.
pub const CHUNK_SIZE: u32 = 64 * 1024;
/// Width and height of the dynamically allocated texture chunks.
pub const TEXTURE_CHUNK_DIMENSIONS: (u16, u16) = (128, 128);
/// Pixel format of the dynamically allocated texture chunks.
pub const TEXTURE_CHUNK_FORMAT: PixelFormat = PixelFormat::Rgba;
