use core::ops::Range;

use super::gen_asset_handle_code;

#[derive(Debug)]
pub struct AudioClipAsset {
    pub samples_per_second: u32,
    pub samples: u32,
    pub chunks: Range<u32>,
}

gen_asset_handle_code!(
    AudioClipAsset,
    AudioClipHandle,
    find_audio_clip,
    get_audio_clip,
    audio_clips
);
