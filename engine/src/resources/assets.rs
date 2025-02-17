pub mod audio_clip;
pub mod texture;

use core::ops::Range;

macro_rules! gen_asset_handle_code {
    ($asset_type:ident, $handle_name:ident, $find_fn:ident, $get_fn:ident, $field:ident) => {
        pub use handle_impl::$handle_name;
        mod handle_impl {
            #[allow(unused_imports)] // used by docs
            use $crate::resources::ResourceDatabase;

            use super::*;

            #[doc = "Handle for [`"]
            #[doc = stringify!($asset_type)]
            #[doc = "`].\n\nCreated with [`"]
            #[doc = concat!("ResourceDatabase::", stringify!($find_fn))]
            #[doc = "`], and can be resolved into a borrow of the asset itself with [`"]
            #[doc = concat!("ResourceDatabase::", stringify!($get_fn))]
            #[doc = "`]."]
            #[derive(Clone, Copy)]
            pub struct $handle_name(usize);
            impl $crate::resources::ResourceDatabase {
                #[doc = "Returns a [`"]
                #[doc = stringify!($handle_name)]
                #[doc = "`] if the database contains a [`"]
                #[doc = stringify!($asset_type)]
                #[doc = "`] with this name. Cache this, and use [`"]
                #[doc = concat!("ResourceDatabase::", stringify!($get_fn))]
                #[doc = "`] to access the actual asset at runtime."]
                pub fn $find_fn(&self, name: &str) -> Option<$handle_name> {
                    let Ok(index) = self
                        .$field
                        .binary_search_by(|asset| asset.name.as_str().cmp(name))
                    else {
                        return None;
                    };
                    Some($handle_name(index))
                }

                #[doc = "Returns the [`"]
                #[doc = stringify!($asset_type)]
                #[doc = "`] behind a handle previously queried with [`"]
                #[doc = concat!("ResourceDatabase::", stringify!($find_fn))]
                #[doc = "`]. Note that reusing handles between separate [`ResourceDatabase`]s will cause panics."]
                pub fn $get_fn(&self, handle: $handle_name) -> &$asset_type {
                    &self.$field[handle.0].asset
                }
            }
        }
    };
}

pub(crate) use gen_asset_handle_code;

/// Trait for operations relevant to any assets, for writing asset management
/// code which is generic over the particular asset type.
pub trait Asset {
    /// If this asset refers to any regular chunks, returns the range
    /// referenced.
    fn get_chunks(&self) -> Option<Range<u32>>;
    /// Applies an offset to all regular chunk references in the asset.
    fn offset_chunks(&mut self, offset: i32);
    /// If this asset refers to any texture chunks, returns the range
    /// referenced.
    fn get_texture_chunks(&self) -> Option<Range<u32>>;
    /// Applies an offset to all texture chunk references in the asset.
    fn offset_texture_chunks(&mut self, offset: i32);
}
