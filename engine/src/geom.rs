// SPDX-FileCopyrightText: 2025 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

/// A floating-point axis-aligned 2D rectangle.
pub struct Rect {
    /// The horizontal coordinate of the top-left corner of the rectangle.
    pub x: f32,
    /// The vertical coordinate of the top-left corner of the rectangle.
    pub y: f32,
    /// The width of the rectangle.
    pub w: f32,
    /// The height of the rectangle.
    pub h: f32,
}

impl Rect {
    /// Creates a new [`Rect`] from a given top-left corner and dimensions.
    pub const fn xywh(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    /// Creates a new [`Rect`] from a given center coordinate and dimensions.
    pub const fn around(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect {
            x: x - w / 2.0,
            y: y - h / 2.0,
            w,
            h,
        }
    }
}
