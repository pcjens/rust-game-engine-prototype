// SPDX-FileCopyrightText: 2025 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

#![allow(warnings)] // TODO: write docs

use core::any::{Any, TypeId};

use arrayvec::ArrayVec;
use bytemuck::Zeroable;

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
    pub fn with_game_object_type<G: GameObject>(&'a mut self, count: usize) -> SceneBuilder<'a> {
        SceneBuilder {
            game_object_infos: GameObjectInfoLinkedList::Element {
                next: &self.game_object_infos,
                info: GameObjectInfo {
                    component_infos: G::component_infos(),
                    game_object_type: G::zeroed().type_id(),
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
                for previous_infos in (self.game_object_infos.into_iter())
                    .take(i + 1)
                    .map(|info| &info.component_infos)
                {
                    for previous_component in previous_infos.iter().take(j) {
                        if component.type_id == previous_component.type_id {
                            already_seen = true;
                            break;
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
            let mut data: FixedVec<u8> = FixedVec::with_alignment(
                arena,
                component_info.size * *total_count,
                component_info.alignment,
            )?;
            data.fill_with_zeroes();
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
            let mut columns = FixedVec::new(arena, component_infos.len())?;
            for component in component_infos {
                let alloc_for_type = {
                    let i = component_datas_by_type
                        .binary_search_by_key(&component.type_id, |(t, _)| *t)
                        .unwrap();
                    &mut component_datas_by_type[i].1
                };
                let data_size = *game_object_count * component.size;
                let data = alloc_for_type.split_off_head(data_size).unwrap();

                let col = ComponentColumn {
                    component_type: component.type_id,
                    data,
                };
                columns.push(col).ok().unwrap();
            }
            let table = GameObjectTable {
                game_object_type: *game_object_type,
                columns,
            };
            game_object_tables.push(table).ok().unwrap();
        }
        game_object_tables.sort_by_key(|table| table.game_object_type);

        Some(Scene { game_object_tables })
    }
}

struct ComponentColumn<'a> {
    component_type: TypeId,
    data: FixedVec<'a, u8>,
}

struct GameObjectTable<'a> {
    game_object_type: TypeId,
    columns: FixedVec<'a, ComponentColumn<'a>>,
}

pub trait GameObject: Zeroable + Any {
    fn component_infos() -> ArrayVec<ComponentInfo, MAX_COMPONENTS>;
}

pub struct Scene<'a> {
    game_object_tables: FixedVec<'a, GameObjectTable<'a>>,
}

#[cfg(test)]
mod tests {
    use core::any::Any;

    use arrayvec::ArrayVec;
    use bytemuck::{Pod, Zeroable};

    use crate::{allocators::LinearAllocator, static_allocator};

    use super::{ComponentInfo, GameObject, Scene, MAX_COMPONENTS};

    #[test]
    fn run_scene_with_manually_typed_out_types() {
        #[derive(Clone, Copy)]
        struct ComponentA {}
        unsafe impl Zeroable for ComponentA {}
        unsafe impl Pod for ComponentA {}

        #[derive(Clone, Copy)]
        struct ComponentB {}
        unsafe impl Zeroable for ComponentB {}
        unsafe impl Pod for ComponentB {}

        struct GameObjectX {
            _a: ComponentA,
        }
        unsafe impl Zeroable for GameObjectX {}

        impl GameObject for GameObjectX {
            fn component_infos() -> ArrayVec<super::ComponentInfo, MAX_COMPONENTS> {
                let mut infos = ArrayVec::new();
                infos.push(ComponentInfo {
                    type_id: ComponentA::zeroed().type_id(),
                    size: size_of::<ComponentA>(),
                    alignment: align_of::<ComponentA>(),
                });
                infos
            }
        }

        struct GameObjectY {
            _a: ComponentA,
            _b: ComponentB,
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
        }

        static ARENA: &LinearAllocator = static_allocator!(10_000);
        let temp_arena = LinearAllocator::new(ARENA, 1000).unwrap();
        let scene = Scene::builder()
            .with_game_object_type::<GameObjectX>(10)
            .with_game_object_type::<GameObjectY>(1)
            .build(ARENA, &temp_arena);
    }
}
