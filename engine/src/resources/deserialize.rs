use core::ops::Range;

use crate::resources::assets::CHUNK_REGION_AUDIO_CLIP_TAG;

use super::{
    AssetIndexHeader, AudioClipAsset, ChunkDescriptor, ChunkRegion, TextureAsset,
    TextureChunkDescriptor,
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

impl Deserialize for ChunkDescriptor<'_> {
    const SERIALIZED_SIZE: usize =
        ChunkRegion::SERIALIZED_SIZE + <Range<u64> as Deserialize>::SERIALIZED_SIZE;
    fn deserialize(src: &[u8]) -> Self {
        assert_eq!(Self::SERIALIZED_SIZE, src.len());
        let mut cursor = 0;
        ChunkDescriptor {
            region: deserialize::<ChunkRegion>(src, &mut cursor),
            source_bytes: deserialize::<Range<u64>>(src, &mut cursor),
            resident: None,
        }
    }
}

impl Deserialize for TextureChunkDescriptor<'_> {
    const SERIALIZED_SIZE: usize =
        u16::SERIALIZED_SIZE * 2 + <Range<u64> as Deserialize>::SERIALIZED_SIZE;
    fn deserialize(src: &[u8]) -> Self {
        assert_eq!(Self::SERIALIZED_SIZE, src.len());
        let mut cursor = 0;
        TextureChunkDescriptor {
            region_width: deserialize::<u16>(src, &mut cursor),
            region_height: deserialize::<u16>(src, &mut cursor),
            source_bytes: deserialize::<Range<u64>>(src, &mut cursor),
            resident: None,
        }
    }
}

impl Deserialize for AssetIndexHeader {
    const SERIALIZED_SIZE: usize = u32::SERIALIZED_SIZE * 4;
    fn deserialize(src: &[u8]) -> Self {
        assert_eq!(Self::SERIALIZED_SIZE, src.len());
        let mut cursor = 0;
        AssetIndexHeader {
            chunks: deserialize::<u32>(src, &mut cursor),
            texture_chunks: deserialize::<u32>(src, &mut cursor),
            textures: deserialize::<u32>(src, &mut cursor),
            audio_clips: deserialize::<u32>(src, &mut cursor),
        }
    }
}

impl Deserialize for AudioClipAsset {
    const SERIALIZED_SIZE: usize =
        u32::SERIALIZED_SIZE * 2 + <Range<u32> as Deserialize>::SERIALIZED_SIZE;
    fn deserialize(src: &[u8]) -> Self {
        assert_eq!(Self::SERIALIZED_SIZE, src.len());
        let mut cursor = 0;
        AudioClipAsset {
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
        TextureAsset {
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
