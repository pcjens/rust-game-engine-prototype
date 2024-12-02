#![no_std]

mod arena;
#[cfg(test)]
mod test_platform;

use core::time::Duration;

use pal::Pal;

pub use arena::{Arena, FixedVec};

/// How much memory is allocated for the frame allocator.
const FRAME_MEM: usize = 1_000_000_000;

pub struct Engine<'platform> {
    platform: &'platform dyn Pal,
    frame_arena: Arena<'platform>,
    test_texture: pal::TextureRef,
}

impl Engine<'_> {
    /// Creates a new instance of the engine.
    pub fn new(platform: &dyn Pal) -> Engine {
        let frame_arena = Arena::new(platform, FRAME_MEM)
            .expect("should have enough memory for the frame allocator");
        let test_texture = platform
            .create_texture(
                2,
                2,
                &mut [
                    0xFF, 0xFF, 0x00, 0xFF, // Yellow
                    0xFF, 0x00, 0xFF, 0xFF, // Pink
                    0x00, 0xFF, 0x00, 0xFF, // Green
                    0x00, 0xFF, 0xFF, 0xFF, // Cyan
                ],
            )
            .unwrap();
        Engine {
            platform,
            frame_arena,
            test_texture,
        }
    }

    /// Runs one iteration of the game loop.
    pub fn iterate(&mut self, _time_since_start: Duration) {
        self.frame_arena.reset();

        let (w, _) = self.platform.draw_area();
        self.platform.draw_triangles(
            &[
                pal::Vertex::new(w / 2. - 200., 200., 0.0, 0.0),
                pal::Vertex::new(w / 2. - 200., 600., 0.0, 1.0),
                pal::Vertex::new(w / 2. + 200., 600., 1.0, 1.0),
                pal::Vertex::new(w / 2. + 200., 200., 1.0, 0.0),
            ],
            &[0, 1, 2, 0, 2, 3],
            pal::DrawSettings {
                texture: Some(self.test_texture),
                ..Default::default()
            },
        );
    }
}
