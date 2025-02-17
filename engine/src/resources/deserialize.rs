use core::{ops::Range, str};

use arrayvec::{ArrayString, ArrayVec};

use super::{
    audio_clip::AudioClipAsset,
    chunks::{ChunkDescriptor, TextureChunkDescriptor},
    texture::{TextureAsset, TextureMipLevel, MAX_MIPS},
    NamedAsset, ResourceDatabaseHeader, ASSET_NAME_LENGTH,
};

/// Trait for describing how a type can be parsed from a constant-size byte
/// slice.
pub trait Deserialize {
    /// The length of the buffer passed into [`Deserialize::deserialize`].
    const SERIALIZED_SIZE: usize;
    /// Deserializes the byte buffer into the struct. The length of `src` must
    /// match the same type's [`Deserialize::SERIALIZED_SIZE`] constant.
    fn deserialize(src: &[u8]) -> Self;
}

impl Deserialize for ChunkDescriptor {
    const SERIALIZED_SIZE: usize = <Range<u64> as Deserialize>::SERIALIZED_SIZE;
    fn deserialize(src: &[u8]) -> Self {
        assert_eq!(Self::SERIALIZED_SIZE, src.len());
        let mut cursor = 0;
        Self {
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

impl Deserialize for ResourceDatabaseHeader {
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
    const SERIALIZED_SIZE: usize = bool::SERIALIZED_SIZE
        + <ArrayVec<TextureMipLevel, MAX_MIPS> as Deserialize>::SERIALIZED_SIZE;
    fn deserialize(src: &[u8]) -> Self {
        assert_eq!(Self::SERIALIZED_SIZE, src.len());
        let mut cursor = 0;
        Self {
            transparent: deserialize::<bool>(src, &mut cursor),
            mip_chain: deserialize::<ArrayVec<TextureMipLevel, MAX_MIPS>>(src, &mut cursor),
        }
    }
}

impl Deserialize for TextureMipLevel {
    // Sadly, `usize::max` is not const. One variant has 4x u16 and 1x u32, the
    // other has 2x u16 and 2x u32, so the max of the two sizes is 12.
    const SERIALIZED_SIZE: usize = bool::SERIALIZED_SIZE + 12;
    fn deserialize(src: &[u8]) -> Self {
        assert_eq!(Self::SERIALIZED_SIZE, src.len());
        let mut cursor = 0;
        let multi_chunk = deserialize::<bool>(src, &mut cursor);
        if multi_chunk {
            Self::MultiChunkTexture {
                size: deserialize::<(u16, u16)>(src, &mut cursor),
                texture_chunks: deserialize::<Range<u32>>(src, &mut cursor),
            }
        } else {
            Self::SingleChunkTexture {
                offset: deserialize::<(u16, u16)>(src, &mut cursor),
                size: deserialize::<(u16, u16)>(src, &mut cursor),
                texture_chunk: deserialize::<u32>(src, &mut cursor),
            }
        }
    }
}

// Serialization helpers, at the bottom because they're very long, just so they
// compile to something sane in debug builds.

/// Deserializes the data from a byte slice into `D`, reading from the given
/// cursor, and advancing it by the amount of bytes read.
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

impl<T: Deserialize, const LEN: usize> Deserialize for ArrayVec<T, LEN> {
    const SERIALIZED_SIZE: usize = u8::SERIALIZED_SIZE + T::SERIALIZED_SIZE * LEN;
    fn deserialize(src: &[u8]) -> Self {
        assert_eq!(Self::SERIALIZED_SIZE, src.len());
        assert!(
            LEN < 0xFF,
            "deserialization impl for ArrayVec only supports lengths up to 255",
        );
        let mut cursor = 0;
        let len = deserialize::<u8>(src, &mut cursor) as usize;
        assert!(len <= LEN, "serialized vec cannot fit");
        let mut vec = ArrayVec::new();
        for _ in 0..len {
            vec.push(deserialize::<T>(src, &mut cursor));
        }
        vec
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

impl Deserialize for (u16, u16) {
    const SERIALIZED_SIZE: usize = u16::SERIALIZED_SIZE * 2;
    #[inline]
    fn deserialize(src: &[u8]) -> Self {
        assert_eq!(Self::SERIALIZED_SIZE, src.len());
        let u0 = u16::deserialize(&src[0..2]);
        let u1 = u16::deserialize(&src[2..4]);
        (u0, u1)
    }
}

impl Deserialize for bool {
    const SERIALIZED_SIZE: usize = 1;
    #[inline]
    fn deserialize(src: &[u8]) -> Self {
        assert_eq!(Self::SERIALIZED_SIZE, src.len());
        // Safety: the the index is checked by the assert above.
        unsafe { *src.get_unchecked(0) != 0 }
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
