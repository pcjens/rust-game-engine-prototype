// SPDX-FileCopyrightText: 2024 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use bytemuck::{Pod, Zeroable};
use engine::{
    allocators::LinearAllocator,
    define_system,
    game_objects::Scene,
    geom::Rect,
    impl_game_object,
    input::{ActionKind, ActionState, InputDeviceState},
    renderer::DrawQueue,
    resources::sprite::SpriteHandle,
    Engine,
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
    let mut engine = Engine::new(&platform, PERSISTENT_ARENA, EngineLimits::DEFAULT);
    let game_arena = LinearAllocator::new(PERSISTENT_ARENA, 8 * 1024 * 1024).unwrap();
    let mut game = Game::new(&game_arena, &engine);
    platform.run_game_loop(&mut engine, |_, platform, engine| {
        run_frame(&mut game, platform, engine);
    });
}

#[cfg(not(any(feature = "sdl2")))]
fn main() {
    compile_error!("at least one of the following platform features is required: 'sdl2'");
}

#[repr(usize)]
enum Input {
    MoveUp,
    MoveDown,
    #[doc(hidden)]
    _Count,
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
struct PlayerMeta {
    device: u64,
}

unsafe impl Zeroable for PlayerMeta {}
unsafe impl Pod for PlayerMeta {}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
struct Position {
    y: i32,
    side: i32,
}

unsafe impl Zeroable for Position {}
unsafe impl Pod for Position {}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
struct Sprite(usize);

unsafe impl Zeroable for Sprite {}
unsafe impl Pod for Sprite {}

#[derive(Debug)]
struct Player {
    tag: PlayerMeta,
    pos: Position,
    sprite: Sprite,
}

impl_game_object! {
    impl GameObject for Player using components {
        tag: PlayerMeta,
        pos: Position,
        sprite: Sprite,
    }
}

#[repr(usize)]
enum SpriteIndex {
    Player = 0,
}

struct Game<'a> {
    player_inputs: Vec<InputDeviceState<{ Input::_Count as usize }>>,
    sprites: Vec<SpriteHandle>,
    next_spawn_side: i32,
    prev_frame: platform::Instant,
    scene: Scene<'a>,
}

impl<'a> Game<'a> {
    fn new(arena: &'a LinearAllocator, engine: &Engine) -> Self {
        let player_sprite = engine.resource_db.find_sprite("player").unwrap();
        let _whack_sound = engine.resource_db.find_audio_clip("whack").unwrap();

        Self {
            player_inputs: Vec::new(),
            sprites: vec![player_sprite],
            scene: Scene::builder()
                .with_game_object_type::<Player>(16)
                .build(arena, &engine.frame_arena)
                .expect("should have enough memory for the test scene"),
            prev_frame: Instant::reference(),
            next_spawn_side: 1,
        }
    }
}

fn run_frame(game: &mut Game, platform: &dyn Platform, engine: &mut Engine) {
    let now = platform.now();
    let delta_millis = now
        .duration_since(game.prev_frame)
        .unwrap()
        .as_millis()
        .min(50) as i32;
    game.prev_frame = now;

    let (screen_width, screen_height) = platform.draw_area();
    let scale_factor = platform.draw_scale_factor();
    let mut draw_queue = DrawQueue::new(&engine.frame_arena, 100, scale_factor).unwrap();

    for event in &*engine.event_queue {
        match event.event {
            Event::DigitalInputPressed(device, _) | Event::DigitalInputReleased(device, _) => {
                {
                    if game
                        .player_inputs
                        .iter()
                        .any(|input| input.device == device)
                    {
                        continue;
                    }

                    game.scene
                        .spawn(Player {
                            tag: PlayerMeta {
                                device: device.inner(),
                            },
                            pos: Position {
                                y: 100,
                                side: game.next_spawn_side,
                            },
                            sprite: Sprite(SpriteIndex::Player as usize),
                        })
                        .unwrap();
                    game.next_spawn_side *= -1;

                    game.player_inputs.push(InputDeviceState {
                        device,
                        actions: [
                            // ExampleInput::MoveUp
                            ActionState {
                                kind: ActionKind::Held,
                                mapping: platform
                                    .default_button_for_action(ActionCategory::Up, device),
                                disabled: false,
                                pressed: false,
                            },
                            // ExampleInput::MoveDown
                            ActionState {
                                kind: ActionKind::Held,
                                mapping: platform
                                    .default_button_for_action(ActionCategory::Down, device),
                                disabled: false,
                                pressed: false,
                            },
                        ],
                    });
                }
            }
        }
    }

    for input in &mut game.player_inputs {
        input.update(&mut engine.event_queue);
    }

    let move_by_input = |players: &mut [PlayerMeta], positions: &mut [Position]| {
        for (player, pos) in players.iter().zip(positions) {
            let Some(input) = game
                .player_inputs
                .iter()
                .find(|i| i.device.inner() == player.device)
            else {
                continue;
            };

            let dy = input.actions[Input::MoveDown as usize].pressed as i32
                - input.actions[Input::MoveUp as usize].pressed as i32;
            pos.y += dy * delta_millis / 2;
            pos.y = pos.y.clamp(0, screen_height as i32);
        }
    };

    game.scene.run_system(define_system!(
        |_, players: &mut [PlayerMeta], positions: &mut [Position]| {
            move_by_input(players, positions);
        }
    ));

    let mut render_sprites = |sprites: &[Sprite], positions: &[Position]| {
        for (sprite, pos) in sprites.iter().zip(positions) {
            let sprite = engine.resource_db.get_sprite(game.sprites[sprite.0]);
            let x = screen_width * (0.5 + -0.4 * pos.side as f32);
            debug_assert!(sprite.draw(
                Rect::around(x, pos.y as f32, 32., 128.),
                0,
                &mut draw_queue,
                &engine.resource_db,
                &mut engine.resource_loader,
            ));
        }
    };

    game.scene.run_system(define_system!(
        |_, sprites: &[Sprite], positions: &[Position]| {
            render_sprites(sprites, positions);
        }
    ));

    draw_queue.dispatch_draw(&engine.frame_arena, platform);
}
