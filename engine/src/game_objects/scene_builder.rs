// SPDX-FileCopyrightText: 2025 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use core::{
    any::TypeId,
    sync::atomic::{AtomicU32, Ordering},
};

use arrayvec::ArrayVec;

use crate::{allocators::LinearAllocator, collections::FixedVec};

use super::{ComponentColumn, ComponentInfo, ComponentVec, GameObject, GameObjectTable, Scene};

struct GameObjectInfo {
    component_infos: ComponentVec<ComponentInfo>,
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

/// Builder for [`Scene`].
pub struct SceneBuilder<'a> {
    game_object_infos: GameObjectInfoLinkedList<'a>,
}

impl<'a> SceneBuilder<'a> {
    /// Adds `G` as a game object type and reserves space for a maximum of
    /// `count` game objects at a time.
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
    /// Creates a [`SceneBuilder`] which is used to create a [`Scene`].
    pub fn builder<'a>() -> SceneBuilder<'a> {
        SceneBuilder {
            game_object_infos: GameObjectInfoLinkedList::End,
        }
    }
}

impl SceneBuilder<'_> {
    /// Allocates memory for and creates a [`Scene`], if `arena` has enough
    /// memory for it.
    ///
    /// The memory requirement of a [`Scene`] is the sum of each component's
    /// size times how many game objects have that component, and possibly
    /// padding bytes between the per-component allocations. Allocations are
    /// done on a per-component basis, so multiple game objects using component
    /// A will simply result in one large allocation for component A that can
    /// fit all of those game objects' components.
    ///
    /// The `temp_arena` allocator is used for small allocations of about 100
    /// bytes per component, and can be reset after this function is done.
    pub fn build<'a>(
        self,
        arena: &'a LinearAllocator,
        temp_arena: &LinearAllocator,
    ) -> Option<Scene<'a>> {
        profiling::function_scope!();

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
            for component_info in component_infos {
                let alloc_for_type = {
                    let i = component_datas_by_type
                        .binary_search_by_key(&component_info.type_id, |(t, _)| *t)
                        .unwrap();
                    &mut component_datas_by_type[i].1
                };
                let data_size = *game_object_count * component_info.size;

                columns.push(ComponentColumn {
                    component_info: *component_info,
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

        // Create a unique id for the scene
        static SCENE_ID_COUNTER: AtomicU32 = AtomicU32::new(0);
        let prev_id = SCENE_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        let scene_id = prev_id.checked_add(1).unwrap();

        Some(Scene {
            id: scene_id,
            generation: 0,
            game_object_tables,
        })
    }
}
