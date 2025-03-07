// SPDX-FileCopyrightText: 2025 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

#![allow(missing_docs)] // TODO: write docs

use core::any::{Any, TypeId};

use arrayvec::ArrayVec;

use crate::{allocators::LinearAllocator, collections::FixedVec};

pub const MAX_COMPONENTS: usize = 32;

pub struct ComponentInfo {
    type_id: TypeId,
    size: usize,
    alignment: usize,
}

struct GameObjectInfo {
    component_infos: ArrayVec<ComponentInfo, MAX_COMPONENTS>,
    game_object_type: TypeId,
    game_object_count: usize,
}

#[allow(clippy::large_enum_variant)]
enum GameObjectInfoLinkedList<'a> {
    End,
    Element {
        next: &'a GameObjectInfoLinkedList<'a>,
        info: GameObjectInfo,
    },
}

impl<'a> IntoIterator for &'a GameObjectInfoLinkedList<'a> {
    type Item = &'a GameObjectInfo;
    type IntoIter = GameObjectInfoLinkedListIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        GameObjectInfoLinkedListIterator { next: self }
    }
}

struct GameObjectInfoLinkedListIterator<'a> {
    next: &'a GameObjectInfoLinkedList<'a>,
}

impl<'a> Iterator for GameObjectInfoLinkedListIterator<'a> {
    type Item = &'a GameObjectInfo;

    fn next(&mut self) -> Option<Self::Item> {
        match self.next {
            GameObjectInfoLinkedList::End => None,
            GameObjectInfoLinkedList::Element { next, info } => {
                self.next = next;
                Some(info)
            }
        }
    }
}

pub struct SceneBuilder<'a> {
    game_object_infos: GameObjectInfoLinkedList<'a>,
}

impl<'a> SceneBuilder<'a> {
    #[track_caller]
    pub fn with_game_object_type<G: GameObject>(&'a mut self, count: usize) -> SceneBuilder<'a> {
        SceneBuilder {
            game_object_infos: GameObjectInfoLinkedList::Element {
                next: &self.game_object_infos,
                info: GameObjectInfo {
                    component_infos: G::component_infos(),
                    game_object_type: TypeId::of::<G>(),
                    game_object_count: count,
                },
            },
        }
    }
}

impl Scene<'_> {
    pub fn builder<'a>() -> SceneBuilder<'a> {
        SceneBuilder {
            game_object_infos: GameObjectInfoLinkedList::End,
        }
    }
}

impl SceneBuilder<'_> {
    pub fn build<'a>(
        self,
        arena: &'a LinearAllocator,
        temp_arena: &LinearAllocator,
    ) -> Option<Scene<'a>> {
        // Count how many component types there are across every game object type
        let mut distinct_components = 0;
        for (i, infos) in (self.game_object_infos.into_iter())
            .enumerate()
            .map(|(i, info)| (i, &info.component_infos))
        {
            for (j, component) in infos.iter().enumerate() {
                let mut already_seen = false;
                'find_prev: for previous_infos in (self.game_object_infos.into_iter())
                    .take(i + 1)
                    .map(|info| &info.component_infos)
                {
                    for previous_component in previous_infos.iter().take(j) {
                        if component.type_id == previous_component.type_id {
                            already_seen = true;
                            break 'find_prev;
                        }
                    }
                }
                if !already_seen {
                    distinct_components += 1;
                }
            }
        }

        // Count how many components there are in total, for each component type
        let mut component_alloc_counts =
            FixedVec::<(&ComponentInfo, usize)>::new(temp_arena, distinct_components)?;
        for game_object_info in &self.game_object_infos {
            for component_info in &game_object_info.component_infos {
                let count = 'count: {
                    for (existing_info, count) in &mut *component_alloc_counts {
                        if component_info.type_id == existing_info.type_id {
                            break 'count count;
                        }
                    }
                    let i = component_alloc_counts.len();
                    component_alloc_counts
                        .push((component_info, 0))
                        .ok()
                        .unwrap();
                    &mut component_alloc_counts[i].1
                };

                *count += game_object_info.game_object_count;
            }
        }

        // Allocate the requested amount of memory for each component type
        let mut component_datas_by_type = FixedVec::new(temp_arena, distinct_components)?;
        for (component_info, total_count) in &*component_alloc_counts {
            let data: FixedVec<u8> = FixedVec::with_alignment(
                arena,
                component_info.size * *total_count,
                component_info.alignment,
            )?;
            component_datas_by_type
                .push((component_info.type_id, data))
                .unwrap();
        }
        component_datas_by_type.sort_unstable_by_key(|(type_id, _)| *type_id);

        // Create the game object tables, using the allocations above as the column data vecs
        let game_object_table_count = self.game_object_infos.into_iter().count();
        let mut game_object_tables = FixedVec::new(arena, game_object_table_count)?;
        for GameObjectInfo {
            component_infos,
            game_object_type,
            game_object_count,
        } in &self.game_object_infos
        {
            let mut columns = ArrayVec::new();
            for component in component_infos {
                let alloc_for_type = {
                    let i = component_datas_by_type
                        .binary_search_by_key(&component.type_id, |(t, _)| *t)
                        .unwrap();
                    &mut component_datas_by_type[i].1
                };
                let data_size = *game_object_count * component.size;

                columns.push(ComponentColumn {
                    component_type: component.type_id,
                    data: alloc_for_type.split_off_head(data_size).unwrap(),
                });
            }

            let table = GameObjectTable {
                game_object_type: *game_object_type,
                columns,
            };
            game_object_tables.push(table).ok().unwrap();
        }
        game_object_tables.sort_unstable_by_key(|table| table.game_object_type);

        Some(Scene { game_object_tables })
    }
}

pub struct ComponentColumn<'a> {
    pub component_type: TypeId,
    pub data: FixedVec<'a, u8>,
}

struct GameObjectTable<'a> {
    game_object_type: TypeId,
    columns: ArrayVec<ComponentColumn<'a>, MAX_COMPONENTS>,
}

pub trait GameObject: Any {
    fn component_infos() -> ArrayVec<ComponentInfo, MAX_COMPONENTS>;
    fn components(&self) -> ArrayVec<(TypeId, &[u8]), MAX_COMPONENTS>;
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
    (gen_closure $table:ident |$param_name:ident: &mut [$param_type:ty]| $func_body:block) => {{
        let type_id = core::any::TypeId::of::<$param_type>();
        let Some(index) = $table.iter().position(|col| col.component_type == type_id) else {
            return;
        };
        let col = $table.remove(index);
        let $param_name: &mut [$param_type] = bytemuck::cast_slice_mut::<u8, $param_type>(&mut col.data);
        $func_body
    }};
    (gen_closure $table:ident |$param_name:ident: &mut [$param_type:ty], $($rest_names:ident: &mut [$rest_types:ty]),+| $func_body:block) => {
        define_system!(gen_closure $table |$($rest_names: &mut [$rest_types]),+| {
            define_system!(gen_closure $table |$param_name: &mut [$param_type]| $func_body)
        })
    };
    (|$($param_name:ident: &mut [$param_type:ty]),+| $func_body:block) => {
        |mut table: ArrayVec<&mut $crate::game_objects::ComponentColumn, {$crate::game_objects::MAX_COMPONENTS}>| {
            define_system!(gen_closure table |$($param_name: &mut [$param_type]),+| $func_body)
        }
    };
}

#[macro_export]
macro_rules! define_game_object {
    (struct_field pub $component_name:ident: $component_type:ty) => {
        pub $component_name: $component_type,
    };
    (struct_fields pub $component_name:ident: $component_type:ty, $(pub $rest_names:ident: $rest_types:ty),+) => {
        define_game_object!(struct_field pub $component_name: $component_type)
        define_game_object!(struct_fields $(pub $rest_names: $res_types),+)
    };

    (struct $type_name:ident {
        $(pub $component_name:ident: $component_type:ty),+
    }) => {
        struct $type_name {
            define_game_object!(struct_fields $(pub $component_name: $component_type),+)
        }

        // TODO: implement GameObject for $type_name
    };
}

#[cfg(test)]
mod tests {
    use core::any::{Any, TypeId};

    use arrayvec::ArrayVec;
    use bytemuck::{Pod, Zeroable};

    use crate::{allocators::LinearAllocator, static_allocator};

    use super::{ComponentInfo, GameObject, Scene, SpawnError, MAX_COMPONENTS};

    #[test]
    fn run_scene_with_manually_typed_out_types() {
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
        unsafe impl Zeroable for GameObjectX {}

        impl GameObject for GameObjectX {
            fn component_infos() -> ArrayVec<ComponentInfo, MAX_COMPONENTS> {
                let mut infos = ArrayVec::new();
                infos.push(ComponentInfo {
                    type_id: ComponentA::zeroed().type_id(),
                    size: size_of::<ComponentA>(),
                    alignment: align_of::<ComponentA>(),
                });
                infos
            }
            fn components(&self) -> ArrayVec<(TypeId, &[u8]), MAX_COMPONENTS> {
                let mut components: ArrayVec<(TypeId, &[u8]), MAX_COMPONENTS> = ArrayVec::new();
                components.push((
                    TypeId::of::<ComponentA>(),
                    bytemuck::cast_ref::<ComponentA, [u8; size_of::<ComponentA>()]>(&self.a),
                ));
                components
            }
        }

        struct GameObjectY {
            a: ComponentA,
            b: ComponentB,
        }
        unsafe impl Zeroable for GameObjectY {}

        impl GameObject for GameObjectY {
            fn component_infos() -> ArrayVec<super::ComponentInfo, MAX_COMPONENTS> {
                let mut infos = ArrayVec::new();
                infos.push(ComponentInfo {
                    type_id: ComponentA::zeroed().type_id(),
                    size: size_of::<ComponentA>(),
                    alignment: align_of::<ComponentA>(),
                });
                infos.push(ComponentInfo {
                    type_id: ComponentB::zeroed().type_id(),
                    size: size_of::<ComponentB>(),
                    alignment: align_of::<ComponentB>(),
                });
                infos
            }
            fn components(&self) -> ArrayVec<(TypeId, &[u8]), MAX_COMPONENTS> {
                let mut components: ArrayVec<(TypeId, &[u8]), MAX_COMPONENTS> = ArrayVec::new();
                components.push((
                    TypeId::of::<ComponentA>(),
                    bytemuck::cast_ref::<ComponentA, [u8; size_of::<ComponentA>()]>(&self.a),
                ));
                components.push((
                    TypeId::of::<ComponentB>(),
                    bytemuck::cast_ref::<ComponentB, [u8; size_of::<ComponentB>()]>(&self.b),
                ));
                components
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

        assert_eq!(Err(SpawnError::NoSpace), scene.spawn(GameObjectX::zeroed()));
        assert_eq!(Err(SpawnError::NoSpace), scene.spawn(GameObjectY::zeroed()));

        let system = define_system!(|a: &mut [ComponentA], b: &mut [ComponentB]| {
            for (a, b) in a.iter_mut().zip(b) {
                a.value += b.value as i64;
            }
        });
        scene.run_system(system);
    }
}
