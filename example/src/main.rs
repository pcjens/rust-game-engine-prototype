// SPDX-FileCopyrightText: 2024 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use engine::{
    allocators::LinearAllocator,
    game_objects::Scene,
    geom::Rect,
    input::{ActionKind, ActionState, InputDeviceState},
    renderer::DrawQueue,
    resources::{audio_clip::AudioClipHandle, sprite::SpriteHandle},
    Engine,
};
use platform::{ActionCategory, Event, Platform};

#[cfg(feature = "sdl2")]
fn main() {
    use engine::{
        allocators::{static_allocator, LinearAllocator},
        Engine, EngineLimits,
    };
    use platform_sdl2::Sdl2Platform;

    #[cfg(feature = "profile")]
    profiling::tracy_client::Client::start();

    let platform = Sdl2Platform::new("example game");
    static PERSISTENT_ARENA: &LinearAllocator = static_allocator!(64 * 1024 * 1024);
    let mut engine = Engine::new(&platform, PERSISTENT_ARENA, EngineLimits::DEFAULT);
    let game_arena = LinearAllocator::new(PERSISTENT_ARENA, 8 * 1024 * 1024).unwrap();
    let mut game = ExampleGame::new(&game_arena, &engine);
    platform.run_game_loop(&mut engine, |_, platform, engine| {
        game.run_frame(platform, engine);
    });
}

#[cfg(not(any(feature = "sdl2")))]
fn main() {
    compile_error!("at least one of the following platform features is required: 'sdl2'");
}

#[repr(usize)]
enum ExampleInput {
    Act,
    _Count,
}

struct ExampleGame<'a> {
    test_input: Option<InputDeviceState<{ ExampleInput::_Count as usize }>>,
    test_sprite: SpriteHandle,
    test_audio: AudioClipHandle,
    test_counter: u32,
    scene: Scene<'a>,
}

impl<'a> ExampleGame<'a> {
    fn new(arena: &'a LinearAllocator, engine: &Engine) -> Self {
        let test_sprite = engine.resource_db.find_sprite("testing sprite").unwrap();
        let test_audio = engine
            .resource_db
            .find_audio_clip("test audio clip")
            .unwrap();
        Self {
            test_input: None,
            test_sprite,
            test_audio,
            test_counter: 0,
            scene: Scene::builder()
                .build(arena, &engine.frame_arena)
                .expect("should have enough memory for the test scene"),
        }
    }

    fn run_frame(&mut self, platform: &dyn Platform, engine: &mut Engine) {
        let scale_factor = platform.draw_scale_factor();
        let mut draw_queue = DrawQueue::new(&engine.frame_arena, 100_000, scale_factor).unwrap();

        let mut action_test = false;

        for event in &*engine.event_queue {
            match event.event {
                Event::DigitalInputPressed(device, _) | Event::DigitalInputReleased(device, _) => {
                    {
                        self.test_input = Some(InputDeviceState {
                            device,
                            actions: [
                                // TestInput::Act
                                ActionState {
                                    kind: ActionKind::Instant,
                                    mapping: platform.default_button_for_action(
                                        ActionCategory::ActPrimary,
                                        device,
                                    ),
                                    disabled: false,
                                    pressed: false,
                                },
                            ],
                        });
                    }
                }
            }
        }

        if let Some(input) = &mut self.test_input {
            input.update(&mut engine.event_queue);
            action_test = input.actions[ExampleInput::Act as usize].pressed;
        }

        if action_test {
            engine
                .audio_mixer
                .play_clip(0, self.test_audio, true, &engine.resource_db);
            self.test_counter += 1;
        }

        self.scene.run_system(|_, _| false);

        let test_sprite = engine.resource_db.get_sprite(self.test_sprite);
        let mut offset = 0.0;
        for mip in 0..9 {
            if self.test_counter % 9 > mip {
                continue;
            }
            let scale = 1. / 2i32.pow(mip) as f32;
            let w = 319.0 * scale;
            let h = 400.0 * scale;
            let draw_success = test_sprite.draw(
                Rect::xywh(offset, 0.0, w, h),
                0,
                &mut draw_queue,
                &engine.resource_db,
                &mut engine.resource_loader,
            );
            assert!(draw_success);
            offset += w + 20.0;
        }

        draw_queue.dispatch_draw(&engine.frame_arena, platform);
    }
}
