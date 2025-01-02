mod audio_clip;
mod texture;

pub use audio_clip::*;
pub use texture::*;

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
            impl $crate::resources::ResourceDatabase<'_> {
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
        }
    };
}

pub(crate) use gen_asset_handle_code;
