mod linear_allocator;
mod static_allocator;

pub use linear_allocator::LinearAllocator;
pub use static_allocator::{static_allocator, StaticAllocator};
