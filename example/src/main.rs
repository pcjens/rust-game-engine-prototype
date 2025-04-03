// SPDX-FileCopyrightText: 2024 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use bytemuck::{Pod, Zeroable};
use engine::{
    allocators::LinearAllocator,
    collections::FixedVec,
    define_system,
    game_objects::Scene,
    geom::Rect,
    impl_game_object,
    input::{ActionKind, ActionState, InputDeviceState},
    renderer::DrawQueue,
    resources::{audio_clip::AudioClipHandle, sprite::SpriteHandle},
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
    platform.run_game_loop(&mut engine, |timestamp, platform, engine| {
        run_frame(timestamp, &mut game, platform, engine);
    });
}

#[cfg(feature = "sdl3")]
fn init() -> (Engine<'static>, Game<'static>) {
    todo!()
}

#[cfg(feature = "sdl3")]
fn iterate(
    timestamp: Instant,
    engine: &mut Engine,
    game: &mut Game,
    platform: &platform_sdl3::Sdl3Platform<Engine, Game>,
) {
    run_frame(timestamp, game, platform, engine);
}

#[cfg(feature = "sdl3")]
platform_sdl3::define_sdl3_main!(Engine<'static>, Game<'static>, init, interate);

#[cfg(not(any(feature = "sdl2", feature = "sdl3")))]
fn main() {
    compile_error!("at least one of the following platform features is required: 'sdl2'");
}

// Components

#[derive(Clone, Copy, Debug)]
#[repr(C)]
struct PlayerMeta {
    device: u64,
}

unsafe impl Zeroable for PlayerMeta {}
unsafe impl Pod for PlayerMeta {}

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
struct Position {
    x: i32,
    y: i32,
}

unsafe impl Zeroable for Position {}
unsafe impl Pod for Position {}
impl Position {
    fn offset(mut self, x: i32, y: i32) -> Self {
        self.x += x;
        self.y += y;
        self
    }
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
struct Collider {
    width: i32,
    height: i32,
}

unsafe impl Zeroable for Collider {}
unsafe impl Pod for Collider {}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
struct Velocity {
    x: i32,
    y: i32,
    acc_x_delta_ms: i32,
    acc_y_delta_ms: i32,
}

unsafe impl Zeroable for Velocity {}
unsafe impl Pod for Velocity {}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
struct Sprite(usize);

unsafe impl Zeroable for Sprite {}
unsafe impl Pod for Sprite {}

// Game objects

#[derive(Debug)]
struct Player {
    tag: PlayerMeta,
    pos: Position,
    sprite: Sprite,
    collider: Collider,
}

impl_game_object! {
    impl GameObject for Player using components {
        tag: PlayerMeta,
        pos: Position,
        sprite: Sprite,
        collider: Collider,
    }
}

#[derive(Debug)]
struct Ball {
    pos: Position,
    sprite: Sprite,
    collider: Collider,
    velocity: Velocity,
}

impl_game_object! {
    impl GameObject for Ball using components {
        pos: Position,
        sprite: Sprite,
        collider: Collider,
        velocity: Velocity,
    }
}

// The rest of the game

#[repr(usize)]
enum Input {
    MoveUp,
    MoveDown,
    Reset,
    #[doc(hidden)]
    _Count,
}

#[repr(usize)]
enum SpriteIndex {
    Square = 0,
}

struct Game<'a> {
    player_inputs: FixedVec<'a, InputDeviceState<{ Input::_Count as usize }>>,
    sprites: FixedVec<'a, SpriteHandle>,
    whack_sound: AudioClipHandle,
    prev_frame: platform::Instant,
    scene: Scene<'a>,
}

impl<'a> Game<'a> {
    fn new(arena: &'a LinearAllocator, engine: &Engine) -> Self {
        let player_sprite = engine.resource_db.find_sprite("player").unwrap();
        let whack_sound = engine.resource_db.find_audio_clip("whack").unwrap();

        let mut sprites = FixedVec::new(arena, 1).unwrap();
        sprites.push(player_sprite).unwrap();

        Self {
            player_inputs: FixedVec::new(arena, 16).unwrap(),
            sprites,
            whack_sound,
            scene: Scene::builder()
                .with_game_object_type::<Player>(16)
                .with_game_object_type::<Ball>(1)
                .build(arena, &engine.frame_arena)
                .expect("should have enough memory for the test scene"),
            prev_frame: Instant::reference(),
        }
    }
}

fn run_frame(timestamp: Instant, game: &mut Game, platform: &dyn Platform, engine: &mut Engine) {
    let delta_millis = timestamp
        .duration_since(game.prev_frame)
        .unwrap()
        .as_millis()
        .min(50) as i32;
    game.prev_frame = timestamp;

    let (screen_width, screen_height) = platform.draw_area();
    let scale_factor = platform.draw_scale_factor();
    let mut draw_queue = DrawQueue::new(&engine.frame_arena, 100, scale_factor).unwrap();

    let mut reset_game_requested = false;
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

                    reset_game_requested = true;
                    let _ = game.player_inputs.push(InputDeviceState {
                        device,
                        actions: [
                            // Input::MoveUp
                            ActionState {
                                kind: ActionKind::Held,
                                mapping: platform
                                    .default_button_for_action(ActionCategory::Up, device),
                                disabled: false,
                                pressed: false,
                            },
                            // Input::MoveDown
                            ActionState {
                                kind: ActionKind::Held,
                                mapping: platform
                                    .default_button_for_action(ActionCategory::Down, device),
                                disabled: false,
                                pressed: false,
                            },
                            // Input::Reset
                            ActionState {
                                kind: ActionKind::Instant,
                                mapping: platform
                                    .default_button_for_action(ActionCategory::Jump, device),
                                disabled: false,
                                pressed: false,
                            },
                        ],
                    });
                }
            }
        }
    }

    for input in game.player_inputs.iter_mut() {
        input.update(&mut engine.event_queue);
    }

    for input in &*game.player_inputs {
        if input.actions[Input::Reset as usize].pressed {
            reset_game_requested = true;
            break;
        }
    }

    if reset_game_requested {
        game.scene.reset();

        let mut next_spawn_side = -1;
        for input in &*game.player_inputs {
            game.scene
                .spawn(Player {
                    tag: PlayerMeta {
                        device: input.device.inner(),
                    },
                    pos: Position {
                        x: screen_width as i32 * (1 + next_spawn_side) / 2,
                        y: screen_height as i32 / 2,
                    },
                    sprite: Sprite(SpriteIndex::Square as usize),
                    collider: Collider {
                        width: 64,
                        height: 128,
                    },
                })
                .unwrap();
            next_spawn_side *= -1;
        }

        game.scene
            .spawn(Ball {
                pos: Position {
                    x: screen_width as i32 / 2,
                    y: screen_height as i32 / 2,
                },
                sprite: Sprite(SpriteIndex::Square as usize),
                collider: Collider {
                    width: 32,
                    height: 32,
                },
                velocity: Velocity {
                    x: 400,
                    y: 400,
                    acc_x_delta_ms: 0,
                    acc_y_delta_ms: 0,
                },
            })
            .unwrap();
    }

    // Player movement
    game.scene.run_system(define_system!(
        |_, players: &mut [PlayerMeta], positions: &mut [Position]| {
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
        }
    ));

    // Collect collider states
    let mut current_colliders = FixedVec::new(&engine.frame_arena, 20).unwrap();
    game.scene.run_system(define_system!(
        |_, positions: &[Position], colliders: &[Collider]| {
            for (pos, collider) in positions.iter().copied().zip(colliders.iter().copied()) {
                current_colliders.push((pos, collider)).unwrap();
            }
        }
    ));
    let did_hit = |collider: Collider, pos: Position, ignore_pos: Position| -> bool {
        if pos.x - collider.width / 2 < 0
            || pos.x + collider.width / 2 >= screen_width as i32
            || pos.y - collider.height / 2 < 0
            || pos.y + collider.height / 2 >= screen_height as i32
        {
            return true;
        }
        for (other_pos, other_collider) in current_colliders.iter() {
            if *other_pos == ignore_pos {
                continue;
            }
            let no_x_overlap =
                (other_pos.x - pos.x).abs() >= other_collider.width / 2 + collider.width / 2;
            let no_y_overlap =
                (other_pos.y - pos.y).abs() >= other_collider.height / 2 + collider.height / 2;
            if !no_x_overlap && !no_y_overlap {
                return true;
            }
        }
        false
    };

    // Move the ball (and bounce off the colliders collected)
    game.scene.run_system(define_system!(
        |_, colliders: &[Collider], positions: &mut [Position], velocities: &mut [Velocity]| {
            for ((collider, pos), vel) in colliders.iter().zip(positions).zip(velocities) {
                vel.acc_x_delta_ms += delta_millis;
                vel.acc_y_delta_ms += delta_millis;
                let dx = vel.x * vel.acc_x_delta_ms / 1000;
                let dy = vel.y * vel.acc_y_delta_ms / 1000;
                vel.acc_x_delta_ms -= dx * 1000 / vel.x;
                vel.acc_y_delta_ms -= dy * 1000 / vel.y;

                if did_hit(*collider, pos.offset(dx, dy), *pos) {
                    if !did_hit(*collider, pos.offset(dx, 0), *pos) {
                        vel.y *= -1;
                        pos.x += dx;
                    } else if !did_hit(*collider, pos.offset(0, dy), *pos) {
                        vel.x *= -1;
                        pos.y += dy;
                    } else {
                        vel.x *= -1;
                        vel.y *= -1;
                    }
                    engine
                        .audio_mixer
                        .play_clip(0, game.whack_sound, true, &engine.resource_db);
                } else {
                    pos.x += dx;
                    pos.y += dy;
                }
            }
        }
    ));

    // Rendering
    game.scene.run_system(define_system!(
        |_, sprites: &[Sprite], positions: &[Position], colliders: &[Collider]| {
            for ((sprite, pos), collider) in sprites.iter().zip(positions).zip(colliders) {
                let sprite = engine.resource_db.get_sprite(game.sprites[sprite.0]);
                debug_assert!(sprite.draw(
                    Rect::around(
                        pos.x as f32,
                        pos.y as f32,
                        collider.width as f32,
                        collider.height as f32,
                    ),
                    0,
                    &mut draw_queue,
                    &engine.resource_db,
                    &mut engine.resource_loader,
                ));
            }
        }
    ));

    draw_queue.dispatch_draw(&engine.frame_arena, platform);
}
