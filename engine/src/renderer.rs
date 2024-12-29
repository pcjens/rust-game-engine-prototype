use platform_abstraction_layer::{BlendMode, DrawSettings, Pal, TextureFilter, TextureRef, Vertex};

use crate::{FixedVec, LinearAllocator};

pub struct TexQuad {
    pub xywh: [f32; 4],
    pub texture_xywh: [f32; 4],
    pub draw_order: u8,
    pub blend_mode: BlendMode,
    pub texture: TextureRef,
}

impl TexQuad {
    fn draw_call_identifier(&self) -> (TextureRef, BlendMode, u8) {
        (self.texture, self.blend_mode, self.draw_order)
    }
}

pub struct DrawQueue<'frm> {
    pub quads: FixedVec<'frm, TexQuad>,
}

impl<'frm> DrawQueue<'frm> {
    pub fn new(allocator: &'frm LinearAllocator) -> Option<DrawQueue<'frm>> {
        Some(DrawQueue {
            quads: FixedVec::new(allocator, 1_000_000)?,
        })
    }

    pub fn dispatch_draw(mut self, allocator: &LinearAllocator, platform: &dyn Pal) {
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

                    let [x, y, w, h] = quad.xywh;
                    let [u, v, tw, th] = quad.texture_xywh;
                    let vert_offset = vertices.len() as u32;
                    let _ = vertices.push(Vertex::new(x, y, u, v));
                    let _ = vertices.push(Vertex::new(x, y + h, u, v + th));
                    let _ = vertices.push(Vertex::new(x + w, y + h, u + tw, v + th));
                    let _ = vertices.push(Vertex::new(x + w, y, u + tw, v));
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
                        texture_filter: TextureFilter::Anisotropic,
                        clip_area: None,
                    },
                );
                vertices.clear();
                indices.clear();
            }
        }
    }
}
