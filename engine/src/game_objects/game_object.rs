// SPDX-FileCopyrightText: 2025 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use core::any::{Any, TypeId};

use arrayvec::ArrayVec;

use super::MAX_COMPONENTS;

pub struct ComponentInfo {
    pub type_id: TypeId,
    pub size: usize,
    pub alignment: usize,
}

pub trait GameObject: Any {
    fn component_infos() -> ArrayVec<ComponentInfo, MAX_COMPONENTS>;
    fn components(&self) -> ArrayVec<(TypeId, &[u8]), MAX_COMPONENTS>;
}

#[macro_export]
macro_rules! impl_game_object {
    // Generators for the GameObject::component_infos implementation
    (/push_info $infos:ident/ $field_type:ty) => {
        $infos.push($crate::game_objects::ComponentInfo {
            type_id: core::any::TypeId::of::<$field_type>(),
            size: core::mem::size_of::<$field_type>(),
            alignment: core::mem::align_of::<$field_type>(),
        });
    };
    (/push_info $infos:ident/ $field_type:ty, $($field_types:ty),+) => {
        impl_game_object!(/push_info $infos/ $field_type);
        impl_game_object!(/push_info $infos/ $($field_types),+);
    };

    // Generators for the GameObject::components implementation
    (/push_component $components:ident, $self:ident/ $field_name:ident: $field_type:ty) => {
        $components.push((
            core::any::TypeId::of::<$field_type>(),
            bytemuck::cast_ref::<$field_type, [u8; size_of::<$field_type>()]>(&$self.$field_name),
        ));
    };
    (/push_component $components:ident, $self:ident/ $field_name:ident: $field_type:ty, $($field_names:ident: $field_types:ty),+) => {
        impl_game_object!(/push_component $components, $self/ $field_name: $field_type);
        impl_game_object!(/push_component $components, $self/ $($field_names: $field_types),+);
    };

    // The main impl-block generator
    (impl GameObject for $struct_name:ident using components {
        $($field_names:ident: $field_types:ty),+,
    }) => {
        impl $crate::game_objects::GameObject for $struct_name {
            fn component_infos(
            ) -> arrayvec::ArrayVec<$crate::game_objects::ComponentInfo, { $crate::game_objects::MAX_COMPONENTS }>
            {
                let mut infos = arrayvec::ArrayVec::new();
                impl_game_object!(/push_info infos/ $($field_types),+);
                infos
            }

            fn components(
                &self,
            ) -> arrayvec::ArrayVec<(core::any::TypeId, &[u8]), { $crate::game_objects::MAX_COMPONENTS }>
            {
                let mut components: arrayvec::ArrayVec<
                    (core::any::TypeId, &[u8]),
                    { $crate::game_objects::MAX_COMPONENTS },
                > = arrayvec::ArrayVec::new();
                impl_game_object!(/push_component components, self/ $($field_names: $field_types),+);
                components
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use core::any::TypeId;

    use bytemuck::from_bytes;

    #[test]
    fn define_game_object_works() {
        struct TestGameObject {
            pub component_a: i32,
            pub component_b: i64,
        }

        impl_game_object! {
            impl GameObject for TestGameObject using components {
                component_a: i32,
                component_b: i64,
            }
        }

        let expected_i32 = i32::MIN;
        let expected_i64 = i64::MAX;
        let game_object = TestGameObject {
            component_a: expected_i32,
            component_b: expected_i64,
        };

        use super::GameObject;
        for (type_id, bytes) in game_object.components() {
            if type_id == TypeId::of::<i32>() {
                let val: &i32 = from_bytes(bytes);
                assert_eq!(expected_i32, *val);
            } else if type_id == TypeId::of::<i64>() {
                let val: &i64 = from_bytes(bytes);
                assert_eq!(expected_i64, *val);
            } else {
                panic!("unrecognized type id from TestGameObject::components");
            }
        }
    }
}
