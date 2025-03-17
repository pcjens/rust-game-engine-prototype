// SPDX-FileCopyrightText: 2024 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::ops::ControlFlow;

use engine::{
    allocators::LinearAllocator,
    geom::Rect,
    input::{ActionKind, ActionState, InputDeviceState},
    renderer::DrawQueue,
    resources::{audio_clip::AudioClipHandle, sprite::SpriteHandle, ResourceDatabase},
    EngineRef, Game,
};
use platform::{ActionCategory, Event, Instant, Platform};

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
    let game_arena = LinearAllocator::new(PERSISTENT_ARENA, 8 * 1024 * 1024).unwrap();
    let mut engine = Engine::<ExampleGame>::new(&platform, PERSISTENT_ARENA, EngineLimits::DEFAULT);
    platform.run_game_loop(&mut engine, game_arena, ());
}

#[cfg(not(any(feature = "sdl2")))]
fn main() {
    compile_error!("at least one of the following platform features is required: 'sdl2'");
}

#[repr(usize)]
enum TestInput {
    Act,
    _Count,
}

struct ExampleGame {
    test_input: Option<InputDeviceState<{ TestInput::_Count as usize }>>,
    test_sprite: SpriteHandle,
    test_audio: AudioClipHandle,
    test_counter: u32,
}

impl Game<'_> for ExampleGame {
    type InitParams = ();

    fn init(
        _params: Self::InitParams,
        _arena: &LinearAllocator,
        resources: &ResourceDatabase,
    ) -> Self {
        let test_sprite = resources.find_sprite("testing sprite").unwrap();
        let test_audio = resources.find_audio_clip("test audio clip").unwrap();
        Self {
            test_input: None,
            test_sprite,
            test_audio,
            test_counter: 0,
        }
    }

    fn run_frame(
        &mut self,
        _timestamp: Instant,
        engine: EngineRef,
        platform: &dyn Platform,
    ) -> ControlFlow<Option<Self::InitParams>> {
        let scale_factor = platform.draw_scale_factor();
        let mut draw_queue = DrawQueue::new(engine.frame_arena, 100_000, scale_factor).unwrap();

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

        // Handle input
        if let Some(input) = &mut self.test_input {
            input.update(engine.event_queue);
            action_test = input.actions[TestInput::Act as usize].pressed;
        }

        if action_test {
            engine
                .audio_mixer
                .play_clip(0, self.test_audio, true, engine.resource_db);
            self.test_counter += 1;
        }

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
                engine.resource_db,
                engine.resource_loader,
            );
            assert!(draw_success);
            offset += w + 20.0;
        }

        draw_queue.dispatch_draw(engine.frame_arena, platform);
        ControlFlow::Continue(())
    }
}
