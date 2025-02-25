// SPDX-FileCopyrightText: 2024 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod linear_allocator;
mod static_allocator;

pub use linear_allocator::LinearAllocator;
pub use static_allocator::{static_allocator, StaticAllocator};
