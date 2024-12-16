use core::ops::Range;

use crate::resources::assets::CHUNK_REGION_AUDIO_CLIP_TAG;

use super::{
    AssetIndexHeader, AudioClipAsset, ChunkDescriptor, ChunkRegion, TextureAsset,
    TextureChunkDescriptor,
};

pub trait Serialize {
    /// The length of the buffer passed into [`Serialize::serialize`].
    const SERIALIZED_SIZE: usize;
    /// Serializes the struct into the byte buffer. The length of `dst` must
    /// match the same type's [`Serialize::SERIALIZED_SIZE`] constant.
    fn serialize(&self, dst: &mut [u8]);
}

impl Serialize for ChunkRegion {
    const SERIALIZED_SIZE: usize = u32::SERIALIZED_SIZE * 2;
    fn serialize(&self, dst: &mut [u8]) {
        assert_eq!(Self::SERIALIZED_SIZE, dst.len());
        let mut cursor = 0;
        match self {
            ChunkRegion::AudioClip {
                start_sample_index,
                samples,
            } => {
                serialize::<u8>(&CHUNK_REGION_AUDIO_CLIP_TAG, dst, &mut cursor);
                serialize::<u32>(start_sample_index, dst, &mut cursor);
                serialize::<u32>(samples, dst, &mut cursor);
            }
        }
    }
}

impl Serialize for ChunkDescriptor<'_> {
    const SERIALIZED_SIZE: usize =
        ChunkRegion::SERIALIZED_SIZE + <Range<u64> as Serialize>::SERIALIZED_SIZE;
    fn serialize(&self, dst: &mut [u8]) {
        assert_eq!(Self::SERIALIZED_SIZE, dst.len());
        let mut cursor = 0;
        let ChunkDescriptor {
            region,
            source_bytes,
            resident: _,
        } = self;
        serialize::<ChunkRegion>(region, dst, &mut cursor);
        serialize::<Range<u64>>(source_bytes, dst, &mut cursor);
    }
}

impl Serialize for TextureChunkDescriptor<'_> {
    const SERIALIZED_SIZE: usize =
        u16::SERIALIZED_SIZE * 2 + <Range<u64> as Serialize>::SERIALIZED_SIZE;
    fn serialize(&self, dst: &mut [u8]) {
        assert_eq!(Self::SERIALIZED_SIZE, dst.len());
        let mut cursor = 0;
        let TextureChunkDescriptor {
            region_width,
            region_height,
            source_bytes,
            resident: _,
        } = self;
        serialize::<u16>(region_width, dst, &mut cursor);
        serialize::<u16>(region_height, dst, &mut cursor);
        serialize::<Range<u64>>(source_bytes, dst, &mut cursor);
    }
}

impl Serialize for AssetIndexHeader {
    const SERIALIZED_SIZE: usize = u32::SERIALIZED_SIZE * 4;
    fn serialize(&self, dst: &mut [u8]) {
        assert_eq!(Self::SERIALIZED_SIZE, dst.len());
        let mut cursor = 0;
        let AssetIndexHeader {
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
    const SERIALIZED_SIZE: usize =
        u16::SERIALIZED_SIZE * 2 + <Range<u32> as Serialize>::SERIALIZED_SIZE;
    fn serialize(&self, dst: &mut [u8]) {
        assert_eq!(Self::SERIALIZED_SIZE, dst.len());
        let mut cursor = 0;
        let TextureAsset {
            width,
            height,
            texture_chunks,
        } = self;
        serialize::<u16>(width, dst, &mut cursor);
        serialize::<u16>(height, dst, &mut cursor);
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
