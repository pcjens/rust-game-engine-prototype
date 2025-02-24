//! Asset type for individual tracks of audio.

use core::ops::Range;

use super::{gen_asset_handle_code, Asset};

gen_asset_handle_code!(
    AudioClipAsset,
    AudioClipHandle,
    find_audio_clip,
    get_audio_clip,
    audio_clips
);

/// Playable audio track.
#[derive(Debug)]
pub struct AudioClipAsset {
    /// The total amount of samples in the chunks.
    pub samples: u32,
    /// The chunks containing the samples.
    pub chunks: Range<u32>,
}

impl Asset for AudioClipAsset {
    fn get_chunks(&self) -> Option<Range<u32>> {
        Some(self.chunks.clone())
    }

    fn offset_chunks(&mut self, offset: i32) {
        self.chunks.start = (self.chunks.start as i32 + offset) as u32;
    }

    fn get_texture_chunks(&self) -> Option<Range<u32>> {
        None
    }

    fn offset_texture_chunks(&mut self, _offset: i32) {}
}
