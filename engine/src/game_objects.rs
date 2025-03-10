// SPDX-FileCopyrightText: 2025 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

#![allow(missing_docs)] // TODO: write docs

mod game_object;
mod scene_builder;

use core::any::TypeId;

use arrayvec::ArrayVec;

use crate::collections::FixedVec;

pub use game_object::{ComponentInfo, GameObject};
pub use scene_builder::SceneBuilder;

pub const MAX_COMPONENTS: usize = 32;

pub struct ComponentColumn<'a> {
    pub component_type: TypeId,
    pub data: FixedVec<'a, u8>,
}

struct GameObjectTable<'a> {
    game_object_type: TypeId,
    columns: ArrayVec<ComponentColumn<'a>, MAX_COMPONENTS>,
}

#[derive(Debug, PartialEq)]
pub enum SpawnError {
    UnregisteredGameObjectType,
    NoSpace,
}

pub struct Scene<'a> {
    game_object_tables: FixedVec<'a, GameObjectTable<'a>>,
}

impl Scene<'_> {
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

    pub fn run_system<F>(&mut self, mut system_func: F)
    where
        F: FnMut(ArrayVec<&mut ComponentColumn, MAX_COMPONENTS>),
    {
        for table in &mut *self.game_object_tables {
            let mut columns = ArrayVec::new();
            for col in &mut *table.columns {
                columns.push(col);
            }
            system_func(columns);
        }
    }
}

#[macro_export]
macro_rules! define_system {
    (/gen_closure $table:ident |$param_name:ident: &mut [$param_type:ty]| $func_body:block) => {{
        let type_id = core::any::TypeId::of::<$param_type>();
        let Some(index) = $table.iter().position(|col| col.component_type == type_id) else {
            return;
        };
        let col = $table.remove(index);
        let $param_name: &mut [$param_type] = bytemuck::cast_slice_mut::<u8, $param_type>(&mut col.data);
        $func_body
    }};
    (/gen_closure $table:ident |$param_name:ident: &mut [$param_type:ty], $($rest_names:ident: &mut [$rest_types:ty]),+| $func_body:block) => {
        define_system!(/gen_closure $table |$($rest_names: &mut [$rest_types]),+| {
            define_system!(/gen_closure $table |$param_name: &mut [$param_type]| $func_body)
        })
    };

    (|$($param_name:ident: &mut [$param_type:ty]),+| $func_body:block) => {
        |mut table: ArrayVec<&mut $crate::game_objects::ComponentColumn, {$crate::game_objects::MAX_COMPONENTS}>| {
            define_system!(/gen_closure table |$($param_name: &mut [$param_type]),+| $func_body)
        }
    };
}

#[cfg(test)]
mod tests {
    use arrayvec::ArrayVec;
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

        struct GameObjectX {
            a: ComponentA,
        }
        impl_game_object! {
            impl GameObject for GameObjectX using components {
                a: ComponentA,
            }
        }

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
