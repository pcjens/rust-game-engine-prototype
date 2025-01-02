use core::ops::Range;

use arrayvec::ArrayString;

use super::{
    assets::{AudioClipAsset, TextureAsset},
    chunks::{ChunkDescriptor, TextureChunkDescriptor},
    NamedAsset, ResourceDatabaseHeader, ASSET_NAME_LENGTH,
};

pub trait Serialize {
    /// The length of the buffer passed into [`Serialize::serialize`].
    const SERIALIZED_SIZE: usize;
    /// Serializes the struct into the byte buffer. The length of `dst` must
    /// match the same type's [`Serialize::SERIALIZED_SIZE`] constant.
    fn serialize(&self, dst: &mut [u8]);
}

impl Serialize for ChunkDescriptor {
    const SERIALIZED_SIZE: usize = <Range<u64> as Serialize>::SERIALIZED_SIZE;
    fn serialize(&self, dst: &mut [u8]) {
        assert_eq!(Self::SERIALIZED_SIZE, dst.len());
        let mut cursor = 0;
        let ChunkDescriptor { source_bytes } = self;
        serialize::<Range<u64>>(source_bytes, dst, &mut cursor);
    }
}

impl Serialize for TextureChunkDescriptor {
    const SERIALIZED_SIZE: usize =
        u16::SERIALIZED_SIZE * 2 + <Range<u64> as Serialize>::SERIALIZED_SIZE;
    fn serialize(&self, dst: &mut [u8]) {
        assert_eq!(Self::SERIALIZED_SIZE, dst.len());
        let mut cursor = 0;
        let TextureChunkDescriptor {
            region_width,
            region_height,
            source_bytes,
        } = self;
        serialize::<u16>(region_width, dst, &mut cursor);
        serialize::<u16>(region_height, dst, &mut cursor);
        serialize::<Range<u64>>(source_bytes, dst, &mut cursor);
    }
}

impl Serialize for ResourceDatabaseHeader {
    const SERIALIZED_SIZE: usize = 13 + u32::SERIALIZED_SIZE * 4;
    fn serialize(&self, dst: &mut [u8]) {
        assert_eq!(Self::SERIALIZED_SIZE, dst.len());
        let mut cursor = 0;

        {
            use crate::resources::*;
            serialize::<u32>(&RESOURCE_DB_MAGIC_NUMBER, dst, &mut cursor);
            serialize::<u32>(&CHUNK_SIZE, dst, &mut cursor);
            serialize::<u16>(&TEXTURE_CHUNK_DIMENSIONS.0, dst, &mut cursor);
            serialize::<u16>(&TEXTURE_CHUNK_DIMENSIONS.1, dst, &mut cursor);
            serialize::<u8>(&(TEXTURE_CHUNK_FORMAT as u8), dst, &mut cursor);
        }

        let ResourceDatabaseHeader {
            chunks,
            texture_chunks,
            textures,
            audio_clips,
        } = self;
        serialize::<u32>(chunks, dst, &mut cursor);
        serialize::<u32>(texture_chunks, dst, &mut cursor);
        serialize::<u32>(textures, dst, &mut cursor);
        serialize::<u32>(audio_clips, dst, &mut cursor);
    }
}

impl<S: Serialize> Serialize for NamedAsset<S> {
    const SERIALIZED_SIZE: usize =
        <ArrayString<ASSET_NAME_LENGTH> as Serialize>::SERIALIZED_SIZE + S::SERIALIZED_SIZE;
    fn serialize(&self, dst: &mut [u8]) {
        assert_eq!(Self::SERIALIZED_SIZE, dst.len());
        let mut cursor = 0;
        let NamedAsset { name, asset } = self;
        serialize::<ArrayString<ASSET_NAME_LENGTH>>(name, dst, &mut cursor);
        serialize::<S>(asset, dst, &mut cursor);
    }
}

impl Serialize for AudioClipAsset {
    const SERIALIZED_SIZE: usize =
        u32::SERIALIZED_SIZE * 2 + <Range<u32> as Serialize>::SERIALIZED_SIZE;
    fn serialize(&self, dst: &mut [u8]) {
        assert_eq!(Self::SERIALIZED_SIZE, dst.len());
        let mut cursor = 0;
        let AudioClipAsset {
            samples_per_second,
            samples,
            chunks,
        } = self;
        serialize::<u32>(samples_per_second, dst, &mut cursor);
        serialize::<u32>(samples, dst, &mut cursor);
        serialize::<Range<u32>>(chunks, dst, &mut cursor);
    }
}

impl Serialize for TextureAsset {
    const SERIALIZED_SIZE: usize = u16::SERIALIZED_SIZE * 2
        + bool::SERIALIZED_SIZE
        + <Range<u32> as Serialize>::SERIALIZED_SIZE;
    fn serialize(&self, dst: &mut [u8]) {
        assert_eq!(Self::SERIALIZED_SIZE, dst.len());
        let mut cursor = 0;
        let TextureAsset {
            width,
            height,
            transparent,
            texture_chunks,
        } = self;
        serialize::<u16>(width, dst, &mut cursor);
        serialize::<u16>(height, dst, &mut cursor);
        serialize::<bool>(transparent, dst, &mut cursor);
        serialize::<Range<u32>>(texture_chunks, dst, &mut cursor);
    }
}

// Serialization helpers, at the bottom because they're very long, just so they
// compile to something sane in debug builds.

#[inline(always)]
pub fn serialize<S: Serialize>(value: &S, dst: &mut [u8], cursor: &mut usize) {
    value.serialize(&mut dst[*cursor..(*cursor + S::SERIALIZED_SIZE)]);
    *cursor += S::SERIALIZED_SIZE;
}

impl<const LEN: usize> Serialize for ArrayString<LEN> {
    const SERIALIZED_SIZE: usize = u8::SERIALIZED_SIZE + LEN;
    fn serialize(&self, dst: &mut [u8]) {
        assert_eq!(Self::SERIALIZED_SIZE, dst.len());
        assert!(
            LEN < 0xFF,
            "deserialization impl for ArrayString only supports string lengths up to 255",
        );
        (self.len() as u8).serialize(&mut dst[0..1]);
        dst[1..1 + self.len()].copy_from_slice(self.as_bytes());
    }
}

impl Serialize for Range<u64> {
    const SERIALIZED_SIZE: usize = u64::SERIALIZED_SIZE * 2;
    #[inline]
    fn serialize(&self, dst: &mut [u8]) {
        assert_eq!(Self::SERIALIZED_SIZE, dst.len());
        self.start.serialize(&mut dst[0..8]);
        self.end.serialize(&mut dst[8..16]);
    }
}

impl Serialize for Range<u32> {
    const SERIALIZED_SIZE: usize = u32::SERIALIZED_SIZE * 2;
    #[inline]
    fn serialize(&self, dst: &mut [u8]) {
        assert_eq!(Self::SERIALIZED_SIZE, dst.len());
        self.start.serialize(&mut dst[0..4]);
        self.end.serialize(&mut dst[4..8]);
    }
}

impl Serialize for bool {
    const SERIALIZED_SIZE: usize = 1;
    #[inline]
    fn serialize(&self, dst: &mut [u8]) {
        assert_eq!(Self::SERIALIZED_SIZE, dst.len());
        let serialized_bool = if *self { 1 } else { 0 };
        // Safety: all the indexes are covered by the assert above.
        unsafe { *dst.get_unchecked_mut(0) = serialized_bool };
    }
}

impl Serialize for u8 {
    const SERIALIZED_SIZE: usize = 1;
    #[inline]
    fn serialize(&self, dst: &mut [u8]) {
        assert_eq!(Self::SERIALIZED_SIZE, dst.len());
        // Safety: all the indexes are covered by the assert above.
        unsafe { *dst.get_unchecked_mut(0) = *self };
    }
}

impl Serialize for u16 {
    const SERIALIZED_SIZE: usize = 2;
    #[inline]
    fn serialize(&self, dst: &mut [u8]) {
        assert_eq!(Self::SERIALIZED_SIZE, dst.len());
        let [a, b] = self.to_le_bytes();
        // Safety: all the indexes are covered by the assert above.
        unsafe {
            *dst.get_unchecked_mut(0) = a;
            *dst.get_unchecked_mut(1) = b;
        }
    }
}

impl Serialize for u32 {
    const SERIALIZED_SIZE: usize = 4;
    #[inline]
    fn serialize(&self, dst: &mut [u8]) {
        assert_eq!(Self::SERIALIZED_SIZE, dst.len());
        let [a, b, c, d] = self.to_le_bytes();
        // Safety: all the indexes are covered by the assert above.
        unsafe {
            *dst.get_unchecked_mut(0) = a;
            *dst.get_unchecked_mut(1) = b;
            *dst.get_unchecked_mut(2) = c;
            *dst.get_unchecked_mut(3) = d;
        }
    }
}

impl Serialize for u64 {
    const SERIALIZED_SIZE: usize = 8;
    #[inline]
    fn serialize(&self, dst: &mut [u8]) {
        assert_eq!(Self::SERIALIZED_SIZE, dst.len());
        let [a, b, c, d, e, f, g, h] = self.to_le_bytes();
        // Safety: all the indexes are covered by the assert above.
        unsafe {
            *dst.get_unchecked_mut(0) = a;
            *dst.get_unchecked_mut(1) = b;
            *dst.get_unchecked_mut(2) = c;
            *dst.get_unchecked_mut(3) = d;
            *dst.get_unchecked_mut(4) = e;
            *dst.get_unchecked_mut(5) = f;
            *dst.get_unchecked_mut(6) = g;
            *dst.get_unchecked_mut(7) = h;
        }
    }
}
