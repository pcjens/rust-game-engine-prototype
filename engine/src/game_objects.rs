// SPDX-FileCopyrightText: 2025 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod game_object;
mod scene_builder;

use core::any::{Any, TypeId};

use arrayvec::ArrayVec;
use bytemuck::Pod;

use crate::collections::FixedVec;

pub use game_object::{ComponentInfo, GameObject};
pub use scene_builder::SceneBuilder;

pub use crate::{define_system, impl_game_object};

/// The maximum amount of components in a [`GameObject`] type.
pub const MAX_COMPONENTS: usize = 32;

/// An [`ArrayVec`] with capacity for [`MAX_COMPONENTS`] elements.
///
/// This exists since these are used throughout the game_objects module, and
/// this allows dependents to e.g. implement the [`GameObject`] trait without
/// depending on [`arrayvec`].
pub type ComponentVec<T> = ArrayVec<T, MAX_COMPONENTS>;

/// Generic storage for the components inside [`Scene`].
///
/// This type generally doesn't need to be interfaced with directly, as
/// [`define_system`] can check and cast these into properly typed slices.
pub struct ComponentColumn<'a> {
    component_type: TypeId,
    data: FixedVec<'a, u8>,
}

impl ComponentColumn<'_> {
    /// Returns the type of the components contained in this struct.
    pub fn component_type(&self) -> TypeId {
        self.component_type
    }

    /// If the [`TypeId`] of `C` is the same as
    /// [`ComponentColumn::component_type`], returns a mutable borrow of the
    /// components in this column.
    ///
    /// This function generally doesn't need to be interfaced with directly, as
    /// [`define_system`] is a straightforward wrapper for iterating through the
    /// columns and calling this on the right one.
    pub fn get_mut<C: Any + Pod>(&mut self) -> Option<&mut [C]> {
        if self.component_type == TypeId::of::<C>() {
            Some(bytemuck::cast_slice_mut::<u8, C>(&mut self.data))
        } else {
            None
        }
    }
}

struct GameObjectTable<'a> {
    game_object_type: TypeId,
    columns: ComponentVec<ComponentColumn<'a>>,
}

/// Error type returned by [`Scene::spawn`].
#[derive(Debug, PartialEq)]
pub enum SpawnError {
    /// Attempted to spawn a game object that wasn't registered for the
    /// [`Scene`] with [`SceneBuilder::with_game_object_type`]. This generally
    /// hints at a bug in the game's scene initialization code.
    UnregisteredGameObjectType,
    /// The [`Scene`]'s storage limit for the [`GameObject`] type has been
    /// reached.
    ///
    /// This can be avoided by reserving more space in the first place (via the
    /// `count` parameter of [`SceneBuilder::with_game_object_type`]), or at
    /// runtime by removing at least one existing game object of the same type
    /// to make room. Game objects can be removed with the [`Scene::retain`]
    /// function.
    NoSpace,
}

/// Container for [`GameObject`]s.
///
/// A scene is initialized with [`Scene::builder`], which is used to register
/// the [`GameObject`] types which can later be spawned into the scene with
/// [`SceneBuilder::with_game_object_type`]. The memory for the game objects is
/// allocated at the end in [`SceneBuilder::build`].
///
/// Game objects are spawned with [`Scene::spawn`], after which they can be
/// accessed by running *systems* (in the Entity-Component-System sense) with
/// [`Scene::run_system`]. To skip the boilerplate, the [`define_system`] macro
/// is recommended for defining system functions.
///
/// ### Example
///
/// ```
/// # static ARENA: &engine::allocators::LinearAllocator = engine::static_allocator!(100_000);
/// # let (arena, temp_arena) = (ARENA, ARENA);
/// use engine::{game_objects::Scene, define_system, impl_game_object};
///
/// // Define some component types:
///
/// // NOTE: Zeroable and Pod are manually implemented here to avoid
/// // the engine depending on proc macros. They should generally be
/// // derived, if compile times allow, as Pod has a lot of
/// // requirements that are easy to forget.
///
/// #[derive(Debug, Clone, Copy)]
/// #[repr(C)]
/// struct Position { pub x: i32, pub y: i32 }
/// unsafe impl bytemuck::Zeroable for Position {}
/// unsafe impl bytemuck::Pod for Position {}
///
/// #[derive(Debug, Clone, Copy)]
/// #[repr(C)]
/// struct Velocity { pub x: i32, pub y: i32 }
/// unsafe impl bytemuck::Zeroable for Velocity {}
/// unsafe impl bytemuck::Pod for Velocity {}
///
/// // Define the "Foo" game object:
///
/// #[derive(Debug)]
/// struct Foo {
///     pub position: Position,
///     pub velocity: Velocity,
/// }
///
/// impl_game_object! {
///     impl GameObject for Foo using components {
///         position: Position,
///         velocity: Velocity,
///     }
/// }
///
/// // Create a Scene that five game objects of type Foo can be spawned in:
/// let mut scene = Scene::builder()
///     .with_game_object_type::<Foo>(5)
///     .build(arena, temp_arena)
///     .unwrap();
///
/// // Spawn a game object of type Foo:
/// scene.spawn(Foo {
///     position: Position { x: 100, y: 100 },
///     velocity: Velocity { x: 20,  y: -10 },
/// }).unwrap();
///
/// // Run a "physics simulation" system for all game objects which
/// // have a Position and Velocity component:
/// scene.run_system(define_system!(|pos: &mut [Position], vel: &mut [Velocity]| {
///     // This closure gets called once for each game object type with a Position
///     // and a Velocity, passing in that type's components, which can be zipped
///     // and iterated through to operate on a single game object's data at a
///     // time. In this case, the closure only gets called for Foo as it's our
///     // only game object type, and these slices are 1 long, as we only spawned
///     // one game object.
///     for (pos, vel) in pos.iter_mut().zip(vel) {
///         pos.x += vel.x;
///         pos.y += vel.y;
///     }
/// }));
///
/// // Just assert that we ended up where we intended to end up.
/// let mut positions_in_scene = 0;
/// scene.run_system(define_system!(|pos: &mut [Position]| {
///     for pos in pos {
///         assert_eq!(120, pos.x);
///         assert_eq!(90, pos.y);
///         positions_in_scene += 1;
///     }
/// }));
/// assert_eq!(1, positions_in_scene);
/// ```
// TODO: figure out how games should approach Scenes' lifetimes
// (and update the above example accordingly)
pub struct Scene<'a> {
    game_object_tables: FixedVec<'a, GameObjectTable<'a>>,
}

impl Scene<'_> {
    /// Spawns the game object into this scene if there's space for it.
    ///
    /// See the [`Scene`] documentation for example usage.
    pub fn spawn<G: GameObject>(&mut self, object: G) -> Result<(), SpawnError> {
        self.spawn_inner(object.type_id(), &object.components())
    }

    fn spawn_inner(
        &mut self,
        game_object_type: TypeId,
        components: &[(TypeId, &[u8])],
    ) -> Result<(), SpawnError> {
        let Some(table) = (self.game_object_tables.iter_mut())
            .find(|table| table.game_object_type == game_object_type)
        else {
            return Err(SpawnError::UnregisteredGameObjectType);
        };

        if table.columns.is_empty() || table.columns[0].data.is_full() {
            return Err(SpawnError::NoSpace);
        }

        for (col, (c_type, c_data)) in table.columns.iter_mut().zip(components) {
            assert_eq!(col.component_type, *c_type);
            let write_succeeded = col.data.extend_from_slice(c_data);
            assert!(write_succeeded, "component should fit");
        }

        Ok(())
    }

    /// Runs `system_func` for each game object type in this [`Scene`], passing
    /// in the components for each.
    ///
    /// Each [`ComponentColumn`] contains tightly packed data for a specific
    /// component type, and the columns can be zipped together to iterate
    /// through sets of components belonging to a single game object, as
    /// component A at index N belongs to the same game object as component B at
    /// index N.
    ///
    /// This is intended to be used with [`define_system`], which can extract
    /// the relevant components from the component columns. See the [`Scene`]
    /// documentation for example usage.
    pub fn run_system<F>(&mut self, mut system_func: F) -> bool
    where
        F: FnMut(ComponentVec<&mut ComponentColumn>) -> bool,
    {
        let mut matched_any_components = false;
        for table in &mut *self.game_object_tables {
            let mut columns = ArrayVec::new();
            for col in &mut *table.columns {
                columns.push(col);
            }
            matched_any_components |= system_func(columns);
        }
        matched_any_components
    }

    #[allow(missing_docs)] // FIXME: remove once designed and implemented
    pub fn retain(&mut self) {
        todo!() // TODO: implement game object removal
    }
}

/// Wraps a closure that takes mut slices of components as parameters, and
/// outputs a closure that can be passed into [`Scene::run_system`].
///
/// The generated closure extracts the relevant component slices from the
/// anonymous [`ComponentColumn`]s, and makes them available to the closure body
/// as variables, using the names from the parameter list.
///
/// For simplicity, the closure passed into this macro can only take mutable
/// slices as parameters, but note that [`Scene::run_system`] takes a [`FnMut`],
/// so the closure can borrow and even mutate their captured environment.
///
/// ### Example
/// ```
/// # static ARENA: &engine::allocators::LinearAllocator = engine::static_allocator!(100_000);
/// # use engine::{game_objects::Scene, define_system, impl_game_object};
/// # #[derive(Debug, Clone, Copy)]
/// # #[repr(C)]
/// # struct Position { pub x: i32, pub y: i32 }
/// # unsafe impl bytemuck::Zeroable for Position {}
/// # unsafe impl bytemuck::Pod for Position {}
/// # #[derive(Debug, Clone, Copy)]
/// # #[repr(C)]
/// # struct Velocity { pub x: i32, pub y: i32 }
/// # unsafe impl bytemuck::Zeroable for Velocity {}
/// # unsafe impl bytemuck::Pod for Velocity {}
/// # let mut scene = Scene::builder().build(ARENA, ARENA).unwrap();
/// scene.run_system(define_system!(|pos: &mut [Position], vel: &mut [Velocity]| {
///     for (pos, vel) in pos.iter_mut().zip(vel) {
///         pos.x += vel.x;
///         pos.y += vel.y;
///     }
/// }));
/// ```
#[macro_export]
macro_rules! define_system {
    (/gen_closure $table:ident |$param_name:ident: &mut [$param_type:ty]| $func_body:block) => {{
        let type_id = core::any::TypeId::of::<$param_type>();
        let Some(index) = $table.iter().position(|col| col.component_type() == type_id) else {
            return false;
        };
        let col = $table.swap_remove(index);
        let $param_name: &mut [$param_type] = col.get_mut().unwrap();
        $func_body
    }};
    (/gen_closure $table:ident |$param_name:ident: &mut [$param_type:ty], $($rest_names:ident: &mut [$rest_types:ty]),+| $func_body:block) => {
        define_system!(/gen_closure $table |$($rest_names: &mut [$rest_types]),+| {
            define_system!(/gen_closure $table |$param_name: &mut [$param_type]| $func_body)
        })
    };

    (|$($param_name:ident: &mut [$param_type:ty]),+| $func_body:block) => {
        |mut table: $crate::game_objects::ComponentVec<&mut $crate::game_objects::ComponentColumn>| {
            define_system!(/gen_closure table |$($param_name: &mut [$param_type]),+| $func_body);
            true
        }
    };
}

#[cfg(test)]
mod tests {
    use bytemuck::{Pod, Zeroable};

    use crate::{allocators::LinearAllocator, impl_game_object, static_allocator};

    use super::{Scene, SpawnError};

    #[test]
    fn run_scene() {
        #[derive(Clone, Copy, Debug)]
        struct ComponentA {
            value: i64,
        }
        unsafe impl Zeroable for ComponentA {}
        unsafe impl Pod for ComponentA {}

        #[derive(Clone, Copy, Debug)]
        struct ComponentB {
            value: u32,
        }
        unsafe impl Zeroable for ComponentB {}
        unsafe impl Pod for ComponentB {}

        #[derive(Debug)]
        struct GameObjectX {
            a: ComponentA,
        }
        impl_game_object! {
            impl GameObject for GameObjectX using components {
                a: ComponentA,
            }
        }

        #[derive(Debug)]
        struct GameObjectY {
            a: ComponentA,
            b: ComponentB,
        }
        impl_game_object! {
            impl GameObject for GameObjectY using components {
                a: ComponentA,
                b: ComponentB,
            }
        }

        static ARENA: &LinearAllocator = static_allocator!(10_000);
        let temp_arena = LinearAllocator::new(ARENA, 1000).unwrap();
        let mut scene = Scene::builder()
            .with_game_object_type::<GameObjectX>(10)
            .with_game_object_type::<GameObjectY>(5)
            .build(ARENA, &temp_arena)
            .unwrap();

        for i in 0..10 {
            let object_x = GameObjectX {
                a: ComponentA { value: i },
            };
            scene.spawn(object_x).unwrap();
        }

        for i in 0..5 {
            let object_y = GameObjectY {
                a: ComponentA { value: -10 },
                b: ComponentB { value: 5 * i },
            };
            scene.spawn(object_y).unwrap();
        }

        assert_eq!(
            Err(SpawnError::NoSpace),
            scene.spawn(GameObjectX {
                a: ComponentA { value: 0 },
            }),
            "the 10 reserved slots for GameObjectX should already be in use",
        );

        assert_eq!(
            Err(SpawnError::NoSpace),
            scene.spawn(GameObjectY {
                a: ComponentA { value: 0 },
                b: ComponentB { value: 0 },
            }),
            "the 5 reserved slots for GameObjectY should already be in use",
        );

        let system = define_system!(|a: &mut [ComponentA], b: &mut [ComponentB]| {
            for (a, b) in a.iter_mut().zip(b) {
                a.value += b.value as i64;
            }
        });
        scene.run_system(system);
    }
}
