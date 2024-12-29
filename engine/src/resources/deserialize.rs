use core::{ops::Range, str};

use arrayvec::ArrayString;

use crate::resources::chunks::CHUNK_REGION_AUDIO_CLIP_TAG;

use super::{
    asset_index::{AssetIndexHeader, NamedAsset, ASSET_NAME_LENGTH},
    assets::{AudioClipAsset, TextureAsset},
    chunks::{ChunkDescriptor, ChunkRegion, TextureChunkDescriptor},
};

pub trait Deserialize {
    /// The length of the buffer passed into [`Deserialize::deserialize`].
    const SERIALIZED_SIZE: usize;
    /// Deserializes the byte buffer into the struct. The length of `src` must
    /// match the same type's [`Deserialize::SERIALIZED_SIZE`] constant.
    fn deserialize(src: &[u8]) -> Self;
}

impl Deserialize for ChunkRegion {
    const SERIALIZED_SIZE: usize = u32::SERIALIZED_SIZE * 2;
    fn deserialize(src: &[u8]) -> Self {
        assert_eq!(Self::SERIALIZED_SIZE, src.len());
        let mut cursor = 0;
        let enum_variant_tag = deserialize::<u8>(src, &mut cursor);
        match enum_variant_tag {
            CHUNK_REGION_AUDIO_CLIP_TAG => {
                let start_sample_index = deserialize::<u32>(src, &mut cursor);
                let samples = deserialize::<u32>(src, &mut cursor);
                ChunkRegion::AudioClip {
                    start_sample_index,
                    samples,
                }
            }
            _ => panic!("tried to deserialize non-existent variant of ChunkRegion"),
        }
    }
}

impl Deserialize for ChunkDescriptor {
    const SERIALIZED_SIZE: usize =
        ChunkRegion::SERIALIZED_SIZE + <Range<u64> as Deserialize>::SERIALIZED_SIZE;
    fn deserialize(src: &[u8]) -> Self {
        assert_eq!(Self::SERIALIZED_SIZE, src.len());
        let mut cursor = 0;
        Self {
            region: deserialize::<ChunkRegion>(src, &mut cursor),
            source_bytes: deserialize::<Range<u64>>(src, &mut cursor),
        }
    }
}

impl Deserialize for TextureChunkDescriptor {
    const SERIALIZED_SIZE: usize =
        u16::SERIALIZED_SIZE * 2 + <Range<u64> as Deserialize>::SERIALIZED_SIZE;
    fn deserialize(src: &[u8]) -> Self {
        assert_eq!(Self::SERIALIZED_SIZE, src.len());
        let mut cursor = 0;
        Self {
            region_width: deserialize::<u16>(src, &mut cursor),
            region_height: deserialize::<u16>(src, &mut cursor),
            source_bytes: deserialize::<Range<u64>>(src, &mut cursor),
        }
    }
}

impl Deserialize for AssetIndexHeader {
    const SERIALIZED_SIZE: usize = 13 + u32::SERIALIZED_SIZE * 4;
    fn deserialize(src: &[u8]) -> Self {
        assert_eq!(Self::SERIALIZED_SIZE, src.len());
        let mut cursor = 0;

        {
            use crate::resources::*;
            let magic = deserialize::<u32>(src, &mut cursor);
            assert_eq!(RESOURCE_DB_MAGIC_NUMBER, magic);
            let chunk_size = deserialize::<u32>(src, &mut cursor);
            assert_eq!(CHUNK_SIZE, chunk_size);
            let texchunk_width = deserialize::<u16>(src, &mut cursor);
            assert_eq!(TEXTURE_CHUNK_DIMENSIONS.0, texchunk_width);
            let texchunk_height = deserialize::<u16>(src, &mut cursor);
            assert_eq!(TEXTURE_CHUNK_DIMENSIONS.1, texchunk_height);
            let texchunk_format = deserialize::<u8>(src, &mut cursor);
            assert_eq!(TEXTURE_CHUNK_FORMAT as u8, texchunk_format);
        }

        Self {
            chunks: deserialize::<u32>(src, &mut cursor),
            texture_chunks: deserialize::<u32>(src, &mut cursor),
            textures: deserialize::<u32>(src, &mut cursor),
            audio_clips: deserialize::<u32>(src, &mut cursor),
        }
    }
}

impl<D: Deserialize> Deserialize for NamedAsset<D> {
    const SERIALIZED_SIZE: usize =
        <ArrayString<ASSET_NAME_LENGTH> as Deserialize>::SERIALIZED_SIZE + D::SERIALIZED_SIZE;
    fn deserialize(src: &[u8]) -> Self {
        assert_eq!(Self::SERIALIZED_SIZE, src.len());
        let mut cursor = 0;
        Self {
            name: deserialize::<ArrayString<ASSET_NAME_LENGTH>>(src, &mut cursor),
            asset: deserialize::<D>(src, &mut cursor),
        }
    }
}

impl Deserialize for AudioClipAsset {
    const SERIALIZED_SIZE: usize =
        u32::SERIALIZED_SIZE * 2 + <Range<u32> as Deserialize>::SERIALIZED_SIZE;
    fn deserialize(src: &[u8]) -> Self {
        assert_eq!(Self::SERIALIZED_SIZE, src.len());
        let mut cursor = 0;
        Self {
            samples_per_second: deserialize::<u32>(src, &mut cursor),
            samples: deserialize::<u32>(src, &mut cursor),
            chunks: deserialize::<Range<u32>>(src, &mut cursor),
        }
    }
}

impl Deserialize for TextureAsset {
    const SERIALIZED_SIZE: usize =
        u16::SERIALIZED_SIZE * 2 + <Range<u32> as Deserialize>::SERIALIZED_SIZE;
    fn deserialize(src: &[u8]) -> Self {
        assert_eq!(Self::SERIALIZED_SIZE, src.len());
        let mut cursor = 0;
        Self {
            width: deserialize::<u16>(src, &mut cursor),
            height: deserialize::<u16>(src, &mut cursor),
            texture_chunks: deserialize::<Range<u32>>(src, &mut cursor),
        }
    }
}

// Serialization helpers, at the bottom because they're very long, just so they
// compile to something sane in debug builds.

#[inline(always)]
pub fn deserialize<D: Deserialize>(src: &[u8], cursor: &mut usize) -> D {
    let value = D::deserialize(&src[*cursor..(*cursor + D::SERIALIZED_SIZE)]);
    *cursor += D::SERIALIZED_SIZE;
    value
}

impl<const LEN: usize> Deserialize for ArrayString<LEN> {
    const SERIALIZED_SIZE: usize = u8::SERIALIZED_SIZE + LEN;
    fn deserialize(src: &[u8]) -> Self {
        assert_eq!(Self::SERIALIZED_SIZE, src.len());
        assert!(
            LEN < 0xFF,
            "deserialization impl for ArrayString only supports string lengths up to 255",
        );
        let len = src[0] as usize;
        assert!(len <= LEN, "serialized string cannot fit");
        ArrayString::from(str::from_utf8(&src[1..1 + len]).unwrap()).unwrap()
    }
}

impl Deserialize for Range<u64> {
    const SERIALIZED_SIZE: usize = u64::SERIALIZED_SIZE * 2;
    #[inline]
    fn deserialize(src: &[u8]) -> Self {
        assert_eq!(Self::SERIALIZED_SIZE, src.len());
        let start = u64::deserialize(&src[0..8]);
        let end = u64::deserialize(&src[8..16]);
        start..end
    }
}

impl Deserialize for Range<u32> {
    const SERIALIZED_SIZE: usize = u32::SERIALIZED_SIZE * 2;
    #[inline]
    fn deserialize(src: &[u8]) -> Self {
        assert_eq!(Self::SERIALIZED_SIZE, src.len());
        let start = u32::deserialize(&src[0..4]);
        let end = u32::deserialize(&src[4..8]);
        start..end
    }
}

impl Deserialize for u8 {
    const SERIALIZED_SIZE: usize = 1;
    #[inline]
    fn deserialize(src: &[u8]) -> Self {
        assert_eq!(Self::SERIALIZED_SIZE, src.len());
        // Safety: the the index is checked by the assert above.
        unsafe { *src.get_unchecked(0) }
    }
}

impl Deserialize for u16 {
    const SERIALIZED_SIZE: usize = 2;
    #[inline]
    fn deserialize(src: &[u8]) -> Self {
        assert_eq!(Self::SERIALIZED_SIZE, src.len());
        // Safety: all the indexes are covered by the assert above.
        let bytes = unsafe { [*src.get_unchecked(0), *src.get_unchecked(1)] };
        u16::from_le_bytes(bytes)
    }
}

impl Deserialize for u32 {
    const SERIALIZED_SIZE: usize = 4;
    #[inline]
    fn deserialize(src: &[u8]) -> Self {
        assert_eq!(Self::SERIALIZED_SIZE, src.len());
        // Safety: all the indexes are covered by the assert above.
        let bytes = unsafe {
            [
                *src.get_unchecked(0),
                *src.get_unchecked(1),
                *src.get_unchecked(2),
                *src.get_unchecked(3),
            ]
        };
        u32::from_le_bytes(bytes)
    }
}

impl Deserialize for u64 {
    const SERIALIZED_SIZE: usize = 8;
    #[inline]
    fn deserialize(src: &[u8]) -> Self {
        assert_eq!(Self::SERIALIZED_SIZE, src.len());
        // Safety: all the indexes are covered by the assert above.
        let bytes = unsafe {
            [
                *src.get_unchecked(0),
                *src.get_unchecked(1),
                *src.get_unchecked(2),
                *src.get_unchecked(3),
                *src.get_unchecked(4),
                *src.get_unchecked(5),
                *src.get_unchecked(6),
                *src.get_unchecked(7),
            ]
        };
        u64::from_le_bytes(bytes)
    }
}
