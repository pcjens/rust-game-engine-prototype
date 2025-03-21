// SPDX-FileCopyrightText: 2024 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod sprite;

use platform::{BlendMode, DrawSettings2D, Platform, SpriteRef, TextureFilter, Vertex2D};

use crate::{allocators::LinearAllocator, collections::FixedVec};

/// Parameters for rendering a sprite.
///
/// Generally created by the engine in e.g.
/// [`SpriteAsset::draw`](crate::resources::sprite::SpriteAsset::draw).
#[derive(Debug)]
pub struct SpriteQuad {
    /// The top-left coordinate of the quad in the same coordinate system as
    /// what [`Platform::draw_area`] returns.
    pub position_top_left: (f32, f32),
    /// The bottom-right coordinate of the quad in the same coordinate system as
    /// what [`Platform::draw_area`] returns.
    pub position_bottom_right: (f32, f32),
    /// The top-left texture coordinate of the quad, each axis between 0..1,
    /// with (0, 0) describing the top-left corner of the texture.
    pub texcoord_top_left: (f32, f32),
    /// The bottom-right texture coordinate of the quad, each axis between 0..1,
    /// with (0, 0) describing the top-left corner of the texture.
    pub texcoord_bottom_right: (f32, f32),
    /// The drawing order of this particular sprite. Sprites with a lower draw
    /// order are rendered below others with a higher one.
    pub draw_order: u8,
    /// The blending mode (if any) to use to draw this sprite above the other
    /// sprites drawn below this one.
    pub blend_mode: BlendMode,
    /// The sprite used to draw this quad with. The region of the sprite used is
    /// controlled with the `texcoord_*` fields.
    pub sprite: SpriteRef,
}

impl SpriteQuad {
    fn draw_call_identifier(&self) -> (SpriteRef, BlendMode, u8) {
        (self.sprite, self.blend_mode, self.draw_order)
    }
}

/// Queue of draw commands to be sorted and shipped off to the platform for
/// rendering and some related rendering state.
///
/// Intended to be recreated every frame, but can be reused between frames to
/// avoid having to queue up the draws again.
pub struct DrawQueue<'frm> {
    /// Sprites to draw.
    pub sprites: FixedVec<'frm, SpriteQuad>,
    /// [`Platform::draw_scale_factor`], stored here because all sprite
    /// rendering needs it, and also has access to the draw queue.
    pub scale_factor: f32,
}

impl<'frm> DrawQueue<'frm> {
    /// Creates a new queue of draws.
    pub fn new(
        allocator: &'frm LinearAllocator,
        max_quads: usize,
        scale_factor: f32,
    ) -> Option<DrawQueue<'frm>> {
        Some(DrawQueue {
            sprites: FixedVec::new(allocator, max_quads)?,
            scale_factor,
        })
    }

    /// Calls the platform draw functions to draw everything queued up until
    /// this point.
    pub fn dispatch_draw(&mut self, allocator: &LinearAllocator, platform: &dyn Platform) {
        'draw_quads: {
            if self.sprites.is_empty() {
                break 'draw_quads;
            }

            self.sprites.sort_unstable_by(|a, b| {
                a.draw_order
                    .cmp(&b.draw_order)
                    .then_with(|| a.sprite.cmp(&b.sprite))
                    .then_with(|| a.blend_mode.cmp(&b.blend_mode))
            });

            let mut max_draw_call_length = 0;
            {
                let mut prev_draw_call_id = None;
                let mut current_draw_call_length = 0;
                for quad in self.sprites.iter() {
                    let current_draw_call_id = Some(quad.draw_call_identifier());
                    if current_draw_call_id == prev_draw_call_id {
                        current_draw_call_length += 1;
                    } else {
                        max_draw_call_length = max_draw_call_length.max(current_draw_call_length);
                        prev_draw_call_id = current_draw_call_id;
                        current_draw_call_length = 1;
                    }
                }
                max_draw_call_length = max_draw_call_length.max(current_draw_call_length);
            }

            let Some(mut vertices) = FixedVec::new(allocator, max_draw_call_length * 4) else {
                break 'draw_quads;
            };
            let Some(mut indices) = FixedVec::new(allocator, max_draw_call_length * 6) else {
                break 'draw_quads;
            };

            let mut quad_i = 0;
            while quad_i < self.sprites.len() {
                // Gather vertices for this draw call
                let current_draw_call_id = self.sprites[quad_i].draw_call_identifier();
                while quad_i < self.sprites.len() {
                    let quad = &self.sprites[quad_i];
                    if quad.draw_call_identifier() != current_draw_call_id {
                        break;
                    }

                    let (x0, y0) = quad.position_top_left;
                    let (x1, y1) = quad.position_bottom_right;
                    let (u0, v0) = quad.texcoord_top_left;
                    let (u1, v1) = quad.texcoord_bottom_right;
                    let vert_offset = vertices.len() as u32;
                    let _ = vertices.push(Vertex2D::new(x0, y0, u0, v0));
                    let _ = vertices.push(Vertex2D::new(x0, y1, u0, v1));
                    let _ = vertices.push(Vertex2D::new(x1, y1, u1, v1));
                    let _ = vertices.push(Vertex2D::new(x1, y0, u1, v0));
                    let _ = indices.push(vert_offset);
                    let _ = indices.push(vert_offset + 1);
                    let _ = indices.push(vert_offset + 2);
                    let _ = indices.push(vert_offset);
                    let _ = indices.push(vert_offset + 2);
                    let _ = indices.push(vert_offset + 3);

                    quad_i += 1;
                }

                // Draw this one
                let (sprite, blend_mode, _) = current_draw_call_id;
                platform.draw_2d(
                    &vertices,
                    &indices,
                    DrawSettings2D {
                        sprite: Some(sprite),
                        blend_mode,
                        texture_filter: TextureFilter::Linear,
                        clip_area: None,
                    },
                );
                vertices.clear();
                indices.clear();
            }
        }
    }
}
