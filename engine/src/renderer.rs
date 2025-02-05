use platform_abstraction_layer::{BlendMode, DrawSettings, Pal, TextureFilter, TextureRef, Vertex};

use crate::{allocators::LinearAllocator, collections::FixedVec};

#[allow(unused_imports)] // used in docs
use crate::resources::assets::TextureAsset;

/// Parameters for rendering a textured quad.
///
/// Generally created by the engine in e.g. [`TextureAsset::draw`].
#[derive(Debug)]
pub struct TexQuad {
    pub position_top_left: (f32, f32),
    pub position_bottom_right: (f32, f32),
    pub texcoord_top_left: (f32, f32),
    pub texcoord_bottom_right: (f32, f32),
    pub draw_order: u8,
    pub blend_mode: BlendMode,
    pub texture: TextureRef,
}

impl TexQuad {
    fn draw_call_identifier(&self) -> (TextureRef, BlendMode, u8) {
        (self.texture, self.blend_mode, self.draw_order)
    }
}

/// Queue of draw commands to be sorted and shipped off to the platform for
/// rendering.
///
/// Intended to be recreated every frame, but can be reused between frames to
/// avoid having to queue up the draws again.
pub struct DrawQueue<'frm> {
    /// Textured quads to draw.
    pub quads: FixedVec<'frm, TexQuad>,
}

impl<'frm> DrawQueue<'frm> {
    /// Creates a new queue of draws.
    pub fn new(allocator: &'frm LinearAllocator, max_quads: usize) -> Option<DrawQueue<'frm>> {
        Some(DrawQueue {
            quads: FixedVec::new(allocator, max_quads)?,
        })
    }

    /// Calls the platform draw functions to draw everything queued up until
    /// this point.
    pub fn dispatch_draw(&mut self, allocator: &LinearAllocator, platform: &dyn Pal) {
        'draw_quads: {
            if self.quads.is_empty() {
                break 'draw_quads;
            }

            self.quads.sort_unstable_by(|a, b| {
                a.draw_order
                    .cmp(&b.draw_order)
                    .then_with(|| a.texture.cmp(&b.texture))
                    .then_with(|| a.blend_mode.cmp(&b.blend_mode))
            });

            let mut max_draw_call_length = 0;
            {
                let mut prev_draw_call_id = None;
                let mut current_draw_call_length = 0;
                for quad in self.quads.iter() {
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
            while quad_i < self.quads.len() {
                // Gather vertices for this draw call
                let current_draw_call_id = self.quads[quad_i].draw_call_identifier();
                while quad_i < self.quads.len() {
                    let quad = &self.quads[quad_i];
                    if quad.draw_call_identifier() != current_draw_call_id {
                        break;
                    }

                    let (x0, y0) = quad.position_top_left;
                    let (x1, y1) = quad.position_bottom_right;
                    let (u0, v0) = quad.texcoord_top_left;
                    let (u1, v1) = quad.texcoord_bottom_right;
                    let vert_offset = vertices.len() as u32;
                    let _ = vertices.push(Vertex::new(x0, y0, u0, v0));
                    let _ = vertices.push(Vertex::new(x0, y1, u0, v1));
                    let _ = vertices.push(Vertex::new(x1, y1, u1, v1));
                    let _ = vertices.push(Vertex::new(x1, y0, u1, v0));
                    let _ = indices.push(vert_offset);
                    let _ = indices.push(vert_offset + 1);
                    let _ = indices.push(vert_offset + 2);
                    let _ = indices.push(vert_offset);
                    let _ = indices.push(vert_offset + 2);
                    let _ = indices.push(vert_offset + 3);

                    quad_i += 1;
                }

                // Draw this one
                let (texture, blend_mode, _) = current_draw_call_id;
                platform.draw_triangles(
                    &vertices,
                    &indices,
                    DrawSettings {
                        texture: Some(texture),
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
