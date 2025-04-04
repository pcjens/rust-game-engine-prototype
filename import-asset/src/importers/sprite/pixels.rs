// SPDX-FileCopyrightText: 2025 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use core::ops::{Index, IndexMut};
use std::fmt::Debug;

use engine::resources::SPRITE_CHUNK_FORMAT;
use platform::PixelFormat;

use super::BPP;

/// Stores a pixel slice and its size and stride for slicing into subregions and
/// blitting between sprites.
pub struct Pixels<'a> {
    pub pixels: &'a mut [u8],
    pub stride: usize,
    pub width: usize,
    pub height: usize,
}

impl Debug for Pixels<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pixels")
            .field("stride", &self.stride)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

impl Pixels<'_> {
    pub fn new(pixels: &mut [u8], stride: usize, width: usize, height: usize) -> Option<Pixels> {
        if pixels.len() != width * BPP + (height - 1) * stride {
            return None;
        }
        Some(Pixels {
            pixels,
            stride,
            width,
            height,
        })
    }

    pub fn subregion(&mut self, x: usize, y: usize, width: usize, height: usize) -> Option<Pixels> {
        if x + width > self.width || y + height > self.height {
            return None;
        }
        let start_index = x * BPP + y * self.stride;
        let end_index = start_index + (height - 1) * self.stride + width * BPP;
        if let Some(subregion) = Pixels::new(
            &mut self.pixels[start_index..end_index],
            self.stride,
            width,
            height,
        ) {
            Some(subregion)
        } else {
            unreachable!("there is a bug in the Pixels::subregion implementation")
        }
    }

    /// Returns a sprite with 2px less width and height, where the outer 1px
    /// edge of the sprite is cropped out.
    ///
    /// Returns None if the sprite is less than 2 pixels wide or high.
    pub fn shrink(&mut self) -> Option<Pixels> {
        if self.width >= 2 && self.height >= 2 {
            if let Some(shrunk) = self.subregion(1, 1, self.width - 2, self.height - 2) {
                Some(shrunk)
            } else {
                unreachable!("there is a bug in the Pixels::shrink implementation")
            }
        } else {
            None
        }
    }

    /// Copies the 1px wide outer edges of the sprite from their neighboring
    /// inner pixels, creating a one-pixel wide "clamp to edge" effect in the
    /// sprite.
    pub fn fill_border(&mut self) {
        // Fill out the left and right edges (ignoring the first and last rows,
        // since they'll be covered after)
        for y in 1..self.height - 1 {
            for c in 0..BPP {
                self[((0, y), c)] = self[((1, y), c)];
                let rightmost_x = self.width - 1;
                self[((rightmost_x, y), c)] = self[((rightmost_x - 1, y), c)];
            }
        }

        // Fill out the top row
        let (first_row, rest) = self.pixels.split_at_mut(self.stride);
        let first_row = &mut first_row[..self.width * BPP];
        let second_row = &rest[..self.width * BPP];
        first_row.copy_from_slice(second_row);

        // Fill out the bottom row
        let (rest, last_row) = self.pixels.split_at_mut((self.height - 1) * self.stride);
        let second_to_last_start = (self.height - 2) * self.stride;
        let second_to_last_row =
            &rest[second_to_last_start..second_to_last_start + self.width * BPP];
        last_row.copy_from_slice(second_to_last_row);
    }

    /// Returns true if the sprite has any transparent pixels.
    pub fn has_transparent_pixels(&self) -> bool {
        assert!(
            matches!(SPRITE_CHUNK_FORMAT, PixelFormat::Rgba),
            "TexPixels::has_transparent_pixels needs updating for the non-rgba sprite chunk format",
        );
        for y in 0..self.height {
            let row = self.row(y);
            let mut alpha_offset = 3;
            while alpha_offset < row.len() {
                if row[alpha_offset] != 0xFF {
                    return true;
                }
                alpha_offset += 4;
            }
        }
        false
    }

    /// Returns a borrow of the pixels at y-coordinate `y`.
    ///
    /// Note that one pixel consists of multiple bytes.
    pub fn row(&self, y: usize) -> &[u8] {
        &self.pixels[y * self.stride..self.width * BPP + y * self.stride]
    }

    /// Returns a mutable borrow of the pixels at y-coordinate `y`.
    ///
    /// Note that one pixel consists of multiple bytes.
    pub fn row_mut(&mut self, y: usize) -> &mut [u8] {
        &mut self.pixels[y * self.stride..self.width * BPP + y * self.stride]
    }

    /// Like copy_from_slice but for sprites.
    /// ### Panics
    /// If the width and height of `self` and `src` don't match.
    pub fn copy_from(&mut self, src: &Pixels) {
        assert_eq!((self.width, self.height), (src.width, src.height));
        for y in 0..self.height {
            self.row_mut(y).copy_from_slice(src.row(y));
        }
    }
}

impl Index<((usize, usize), usize)> for Pixels<'_> {
    type Output = u8;
    #[inline(always)]
    fn index(&self, ((x, y), channel): ((usize, usize), usize)) -> &Self::Output {
        &self.pixels[channel + x * BPP + y * self.stride]
    }
}

impl IndexMut<((usize, usize), usize)> for Pixels<'_> {
    #[inline(always)]
    fn index_mut(&mut self, ((x, y), channel): ((usize, usize), usize)) -> &mut Self::Output {
        &mut self.pixels[channel + x * BPP + y * self.stride]
    }
}

#[cfg(test)]
mod tests {
    use engine::resources::SPRITE_CHUNK_FORMAT;

    use super::Pixels;

    const BPP: usize = SPRITE_CHUNK_FORMAT.bytes_per_pixel();

    #[test]
    fn sprite_util_subregion_length_looks_right() {
        let mut pixels = [0; 4 * 4 * BPP];
        let stride = 4 * BPP;
        let mut sprite = Pixels::new(&mut pixels, stride, 4, 4).unwrap();

        let region = sprite.subregion(1, 1, 3, 3).unwrap();
        assert_eq!(stride * 2 + 3 * BPP, region.pixels.len());

        let region = sprite.subregion(0, 0, 2, 2).unwrap();
        assert_eq!(stride + 2 * BPP, region.pixels.len());
    }

    #[test]
    fn sprite_util_subregion_first_pixel_looks_right() {
        let mut pixels = [0; 4 * 4 * BPP];
        let stride = 4 * BPP;
        pixels[2 * BPP + 2 * stride] = 0xFF; // write FF at (2, 2) channel 0
        let mut sprite = Pixels::new(&mut pixels, stride, 4, 4).unwrap();

        let region = sprite.subregion(1, 1, 3, 3).unwrap();
        let value_at_1_1_0 = region[((1, 1), 0)];
        assert_eq!(0xFF, value_at_1_1_0);
    }

    #[test]
    fn sprite_util_fill_border_and_shrink_work() {
        let mut pixels: [u8; 4 * 4 * BPP] = [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, //
            0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 0, 0, 0, 0, //
            0, 0, 0, 0, 2, 1, 4, 2, 6, 5, 8, 7, 0, 0, 0, 0, //
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, //
        ];
        const TARGET_INNER_PIXELS: [u8; 2 * 2 * BPP] = [
            1, 2, 3, 4, 5, 6, 7, 8, //
            2, 1, 4, 2, 6, 5, 8, 7, //
        ];
        const TARGET_PIXELS: [u8; 4 * 4 * BPP] = [
            1, 2, 3, 4, 1, 2, 3, 4, 5, 6, 7, 8, 5, 6, 7, 8, //
            1, 2, 3, 4, 1, 2, 3, 4, 5, 6, 7, 8, 5, 6, 7, 8, //
            2, 1, 4, 2, 2, 1, 4, 2, 6, 5, 8, 7, 6, 5, 8, 7, //
            2, 1, 4, 2, 2, 1, 4, 2, 6, 5, 8, 7, 6, 5, 8, 7, //
        ];

        let mut sprite = Pixels::new(&mut pixels, 4 * BPP, 4, 4).unwrap();
        sprite.fill_border();
        assert_eq!(&TARGET_PIXELS, sprite.pixels);

        let shrunk = sprite.shrink().unwrap();
        assert_eq!(2, shrunk.width);
        assert_eq!(2, shrunk.height);
        for y in 0..2 {
            for x in 0..2 {
                for c in 0..BPP {
                    assert_eq!(
                        TARGET_INNER_PIXELS[c + x * BPP + y * 2 * BPP],
                        shrunk[((x, y), c)]
                    );
                }
            }
        }
    }
}
