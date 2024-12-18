use core::cmp::Ordering;

use arrayvec::ArrayString;

pub const ASSET_NAME_LENGTH: usize = 27;

/// Wrapper for assets with their unique name. Implement equality and comparison
/// operators purely based on the name, as assets with a specific name should be
/// unique within a resource database.
pub struct NamedAsset<T> {
    pub name: ArrayString<ASSET_NAME_LENGTH>,
    pub asset: T,
}

impl<T> PartialEq for NamedAsset<T> {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

// The equality operator just checks the name, and ArrayString is Eq.
impl<T> Eq for NamedAsset<T> {}

impl<T> PartialOrd for NamedAsset<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for NamedAsset<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.name.cmp(&other.name)
    }
}
