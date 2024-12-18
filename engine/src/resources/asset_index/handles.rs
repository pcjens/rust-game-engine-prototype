use crate::resources::TextureAsset;

use super::{AssetIndex, AudioClipAsset};

macro_rules! gen_asset_handle_code {
    ($asset_type:ident, $handle_name:ident, $find_fn:ident, $get_fn:ident, $field:ident) => {
        pub struct $handle_name(usize);
        impl AssetIndex<'_> {
            pub fn $find_fn(&self, name: &str) -> Option<$handle_name> {
                let Ok(index) = self
                    .$field
                    .binary_search_by(|asset| asset.name.as_str().cmp(name))
                else {
                    return None;
                };
                Some($handle_name(index))
            }
            pub fn $get_fn(&self, handle: $handle_name) -> &$asset_type {
                &self.$field[handle.0].asset
            }
        }
    };
}

gen_asset_handle_code!(
    AudioClipAsset,
    AudioClipHandle,
    find_audio_clip,
    get_audio_clip,
    audio_clips
);

gen_asset_handle_code!(
    TextureAsset,
    TextureHandle,
    find_texture,
    get_texture,
    textures
);
