// SPDX-FileCopyrightText: 2025 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//!
//! This module makes wide use of macros, which means that there's quite a bit
//! of the "plumbing" exposed in this module. The main parts to look into are
//! [`Scene`](crate::game_objects::Scene), [`define_system`], and
//! [`impl_game_object`].

mod game_object;
mod scene_builder;

use core::{
    any::{Any, TypeId},
    cmp::{Ordering, Reverse},
};

use arrayvec::ArrayVec;
use bytemuck::Pod;

use crate::collections::FixedVec;

pub use game_object::{impl_game_object, ComponentInfo, GameObject};
pub use scene_builder::SceneBuilder;

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
    component_info: ComponentInfo,
    data: FixedVec<'a, u8>,
}

impl ComponentColumn<'_> {
    /// Returns the type of the components contained in this struct.
    pub fn component_type(&self) -> TypeId {
        self.component_info.type_id
    }

    /// If the [`TypeId`] of `C` is the same as
    /// [`ComponentColumn::component_type`], returns a mutable borrow of the
    /// components in this column.
    ///
    /// This function generally doesn't need to be interfaced with directly, as
    /// [`define_system`] is a straightforward wrapper for iterating through the
    /// columns and calling this on the right one.
    pub fn get_mut<C: Any + Pod>(&mut self) -> Option<&mut [C]> {
        if self.component_info.type_id == TypeId::of::<C>() {
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

impl GameObjectTable<'_> {
    /// Swaps the components in all component between the first and second
    /// index.
    ///
    /// If the indices are the same, this does nothing.
    fn swap(&mut self, index_a: usize, index_b: usize) {
        match index_a.cmp(&index_b) {
            Ordering::Equal => {}
            Ordering::Greater => self.swap(index_b, index_a),
            Ordering::Less => {
                for col in &mut self.columns {
                    let size = col.component_info.size;
                    let a_byte_index = size * index_a;
                    let b_byte_index = size * index_b;
                    let (contains_a, starts_with_b) = col.data.split_at_mut(b_byte_index);
                    let a = &mut contains_a[a_byte_index..a_byte_index + size];
                    let b = &mut starts_with_b[..size];
                    a.swap_with_slice(b);
                }
            }
        }
    }

    /// Truncates the component columns to only contain `new_len` components,
    /// i.e. deletes game objects from the end of this table to have `new_len`
    /// game objects at maximum.
    fn truncate(&mut self, new_len: usize) {
        for col in &mut self.columns {
            let new_bytes_len = new_len * col.component_info.size;
            col.data.truncate(new_bytes_len);
        }
    }

    fn len(&self) -> usize {
        if self.columns.is_empty() {
            0
        } else {
            let col = &self.columns[0];
            col.data.len() / col.component_info.size
        }
    }
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
    /// to make room. Game objects can be removed with the [`Scene::delete`]
    /// function.
    NoSpace,
}

/// Temporary handle for operating on specific game objects. Invalidated by
/// [`Scene::delete`].
///
/// After invalidation, these handles don't refer to anything.
#[derive(Clone, Copy, Debug)]
pub struct GameObjectHandle {
    scene_id: u32,
    scene_generation: u64,
    game_object_table_index: u32,
    game_object_index: usize,
}

/// Returns [`GameObjectHandle`]s for referring to the game objects in e.g.
/// [`Scene::delete`].
///
/// When iterated alongside the component slices in [`Scene::run_system`] (e.g.
/// with a [`Zip`](core::iter::Zip) iterator), the returned handles correspond
/// to the game objects whose components are being used on any particular
/// iteration.
pub struct GameObjectHandleIterator {
    scene_id: u32,
    scene_generation: u64,
    game_object_table_index: u32,
    next_game_object_index: usize,
    total_game_objects: usize,
}

impl Iterator for GameObjectHandleIterator {
    type Item = GameObjectHandle;
    fn next(&mut self) -> Option<Self::Item> {
        if self.next_game_object_index < self.total_game_objects {
            let game_object_index = self.next_game_object_index;
            self.next_game_object_index += 1;
            Some(GameObjectHandle {
                scene_id: self.scene_id,
                scene_generation: self.scene_generation,
                game_object_table_index: self.game_object_table_index,
                game_object_index,
            })
        } else {
            None
        }
    }
}

/// Container for [`GameObject`]s.
///
/// A scene is initialized with [`Scene::builder`], which is used to register
/// the [`GameObject`] types which can be spawned into the scene. The memory for
/// the game objects is allocated at the end in [`SceneBuilder::build`].
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
/// scene.run_system(define_system!(|_| |pos: &mut [Position], vel: &[Velocity]| {
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
/// scene.run_system(define_system!(|_| |pos: &[Position]| {
///     for pos in pos {
///         assert_eq!(120, pos.x);
///         assert_eq!(90, pos.y);
///         positions_in_scene += 1;
///     }
/// }));
/// assert_eq!(1, positions_in_scene);
///
/// // Game objects can be deleted by collecting and deleting them in batches:
/// use engine::collections::FixedVec;
/// let mut handles_to_delete = FixedVec::new(temp_arena, 1).unwrap();
/// scene.run_system(define_system!(|handles| |pos: &[Position]| {
///     for (handle, pos) in handles.zip(pos) {
///         if pos.x == 120 {
///             handles_to_delete.push(handle).unwrap();
///         }
///     }
/// }));
///
/// // NOTE: After deletion, all handles get invalidated, so
/// // handles_to_delete would need to be re-acquired from a run_system call.
/// scene.delete(&mut handles_to_delete).unwrap();
/// ```
// TODO: figure out how games should approach Scenes' lifetimes
// (and update the above example accordingly)
pub struct Scene<'a> {
    /// A unique identifier for distinguishing between [`GameObjectHandle`]s
    /// acquired from different scenes.
    id: u32,
    /// An incrementing value for detecting invalidated [`GameObjectHandle`]s.
    /// Incremented whenever indexes to game_object_tables or the tables' inner
    /// vecs are invalidated.
    generation: u64,
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
            assert_eq!(col.component_info.type_id, *c_type);
            let write_succeeded = col.data.extend_from_slice(c_data);
            assert!(write_succeeded, "component should fit");
        }

        Ok(())
    }

    /// Runs `system_func` for each game object type in this [`Scene`], passing
    /// in the components for each.
    ///
    /// Returns `false` if all `system_func` invocations return `false`. When
    /// using [`define_system`], this happens when the scene doesn't contain any
    /// game object types with the set of components requested.
    ///
    /// Each [`ComponentColumn`] contains tightly packed data for a specific
    /// component type, and the columns can be zipped together to iterate
    /// through sets of components belonging to a single game object, as
    /// component A at index N belongs to the same game object as component B at
    /// index N.
    ///
    /// The [`GameObjectHandleIterator`] returns handles to the game objects
    /// associated with the components in a particular iteration, if iterated
    /// through at the same pace as the component columns.
    ///
    /// This is intended to be used with [`define_system`], which can extract
    /// the relevant components from the component columns. See the [`Scene`]
    /// documentation for example usage.
    pub fn run_system<F>(&mut self, mut system_func: F) -> bool
    where
        F: FnMut(GameObjectHandleIterator, ComponentVec<&mut ComponentColumn>) -> bool,
    {
        let mut matched_any_components = false;
        for (table_index, table) in self.game_object_tables.iter_mut().enumerate() {
            let handle_iter = GameObjectHandleIterator {
                scene_id: self.id,
                scene_generation: self.generation,
                game_object_table_index: table_index as u32,
                next_game_object_index: 0,
                total_game_objects: table.len(),
            };

            let mut columns = ArrayVec::new();
            for col in &mut *table.columns {
                columns.push(col);
            }

            matched_any_components |= system_func(handle_iter, columns);
        }
        matched_any_components
    }

    /// Deletes the game objects referred to by the given handles.
    ///
    /// If any handles are invalid (e.g. have been invalidated by a previous
    /// call to [`Scene::delete`]), the amount of invalid handles is returned in
    /// an Err.
    ///
    /// The slice of handles is mutable to allow sorting the slice, which is
    /// needed for a performant implementation of this function.
    pub fn delete(&mut self, handles: &mut [GameObjectHandle]) -> Result<(), usize> {
        let mut invalid_handles = 0;

        // Sort the handles, so that deletions are grouped by table index (not
        // necessary for the algorithm, but seems a bit better for data
        // locality), and the individual game object indices are processed in
        // descending order from the end (which allows deleting by
        // swap-and-truncate without invalidating any future indexes to delete).
        handles.sort_unstable_by_key(|handle| {
            (
                handle.game_object_table_index,
                Reverse(handle.game_object_index),
            )
        });

        for handle in handles {
            if handle.scene_id != self.id || handle.scene_generation != self.generation {
                invalid_handles += 1;
                continue;
            }

            let table = &mut self.game_object_tables[handle.game_object_table_index as usize];
            let table_last_index = table.len() - 1;
            table.swap(handle.game_object_index, table_last_index);
            table.truncate(table_last_index);
        }

        self.generation += 1;

        if invalid_handles == 0 {
            Ok(())
        } else {
            Err(invalid_handles)
        }
    }
}

/// Searches the columns for one containing components of type `C`, and returns
/// it as a properly typed slice.
pub fn extract_component_column<'a, C: Pod + Any>(
    columns: &mut ComponentVec<&'a mut ComponentColumn>,
) -> Option<&'a mut [C]> {
    let index = columns
        .iter()
        .position(|col| col.component_type() == TypeId::of::<C>())?;
    let col = columns.swap_remove(index);
    Some(col.get_mut().unwrap())
}

/// Gutputs a closure that can be passed into [`Scene::run_system`], handling
/// extracting properly typed component columns based on the parameter list.
///
/// This macro only outputs a single closure with no inner closures, the pattern
/// simply follows the syntax of closures to make the formatter happy and the
/// code easy to read.
///
/// The generated closure extracts the relevant component slices from the
/// anonymous [`ComponentColumn`]s, and makes them available to the closure body
/// as variables, using the names from the parameter list.
///
/// Similarly, the [`GameObjectHandleIterator`] parameter from
/// [`Scene::run_system`] is assigned to the pattern given as the parameter in
/// the outer closure. Usually the pattern is either a variable name, or an
/// underscore if the handles aren't needed.
///
/// For simplicity, the inner closure passed into this macro can only take
/// mutable slices as parameters, but note that [`Scene::run_system`] takes a
/// [`FnMut`], so the closure can borrow and even mutate their captured
/// environment.
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
/// let mut game_object_handle = None;
/// scene.run_system(define_system!(|handles| |pos: &mut [Position], vel: &[Velocity]| {
///     for ((handle, pos), vel) in handles.zip(pos).zip(vel) {
///         pos.x += vel.x;
///         pos.y += vel.y;
///         game_object_handle = Some(handle);
///     }
/// }));
/// if let Some(handle) = game_object_handle {
///     scene.delete(&mut [handle]).unwrap();
/// }
/// ```
#[macro_export]
macro_rules! define_system {
    (/param_defs/ $table:ident / $func_body:block / |$param_name:ident: $param_type:ty|) => {{
        let col: Option<&mut [_]> = $crate::game_objects::extract_component_column(&mut $table);
        let Some(col) = col else {
            return false;
        };
        let $param_name: $param_type = col;
        $func_body
    }};
    (/param_defs/ $table:ident / $func_body:block / |$param_name:ident: $param_type:ty, $($rest_names:ident: $rest_types:ty),+|) => {
        define_system!(/param_defs/ $table / {
            define_system!(/param_defs/ $table / $func_body / |$param_name: $param_type|)
        } / |$($rest_names: $rest_types),+|)
    };

    (|$handle_name:pat_param| |$($param_name:ident: $param_type:ty),+| $func_body:block) => {
        |#[allow(unused_variables)] handle_iter: $crate::game_objects::GameObjectHandleIterator,
         mut table: $crate::game_objects::ComponentVec<&mut $crate::game_objects::ComponentColumn>| {
            let $handle_name = handle_iter;
            define_system!(/param_defs/ table / $func_body / |$($param_name: $param_type),+|);
            true
        }
    };
}

pub use define_system;

#[cfg(test)]
mod tests {
    use arrayvec::ArrayVec;
    use bytemuck::{Pod, Zeroable};

    use crate::{
        allocators::LinearAllocator, game_objects::GameObjectHandle, impl_game_object,
        static_allocator,
    };

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
                a: ComponentA { value: -i },
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

        // Assert that there aren't any ComponentA's with positive values:
        let mut processed_count = 0;
        scene.run_system(define_system!(|_| |a: &[ComponentA]| {
            for a in a {
                assert!(a.value <= 0);
                processed_count += 1;
            }
        }));
        assert!(processed_count > 0);

        // Apply some changes to GameObjectY's:
        let system = define_system!(|_| |a: &mut [ComponentA], b: &[ComponentB]| {
            for (a, b) in a.iter_mut().zip(b) {
                a.value += b.value as i64;
            }
        });
        scene.run_system(system);

        // Assert that there *are* positive values now, and delete them:
        let mut processed_count = 0;
        let mut handles_to_delete: ArrayVec<GameObjectHandle, 15> = ArrayVec::new();
        scene.run_system(define_system!(|handles| |a: &[ComponentA]| {
            for (handle, a) in handles.zip(a) {
                if a.value > 0 {
                    handles_to_delete.push(handle);
                }
                processed_count += 1;
            }
        }));
        scene.delete(&mut handles_to_delete).unwrap();
        assert!(processed_count > 0);

        // Assert that there aren't any positive values anymore, now that they
        // were deleted:
        let mut processed_count = 0;
        scene.run_system(define_system!(|_| |a: &[ComponentA]| {
            for a in a {
                assert!(a.value <= 0);
                processed_count += 1;
            }
        }));
        assert!(processed_count > 0);
    }
}
