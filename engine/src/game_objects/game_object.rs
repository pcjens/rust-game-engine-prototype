// SPDX-FileCopyrightText: 2025 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use core::{
    any::{Any, TypeId},
    fmt::Debug,
};

use super::ComponentVec;

/// Type description for allocation and type comparison of components. Generated
/// by [`impl_game_object`](super::impl_game_object).
#[derive(Clone, Copy)]
pub struct ComponentInfo {
    /// The type of the component. Eventually passed into a
    /// [`ComponentColumn`](super::ComponentColumn), and returned from
    /// [`ComponentColumn::component_type`](super::ComponentColumn::component_type).
    pub type_id: TypeId,
    /// The [size_of] the component type.
    pub size: usize,
    /// The [align_of] the component type.
    pub alignment: usize,
}

/// Trait that game object types implement to be able to be added to a
/// [`Scene`](super::Scene). Impl generated with
/// [`impl_game_object`](super::impl_game_object).
pub trait GameObject: Any + Debug {
    /// Returns the allocation and type comparison details for the components of
    /// this game object type.
    ///
    /// The order of the infos is the same as [`GameObject::components`].
    fn component_infos() -> ComponentVec<ComponentInfo>;
    /// Returns a single game object's components as anonymous byte slices, with
    /// the type id for component type detection.
    ///
    /// The order of the components is the same as
    /// [`GameObject::component_infos`].
    fn components(&self) -> ComponentVec<(TypeId, &[u8])>;
}

/// Generates a [`GameObject`] impl block for a type.
///
/// This takes a list of the struct's field names and types to be used as the
/// components of this game object. Note that component types must be
/// [`bytemuck::Pod`].
///
/// The `using components` part is intended to signal that it's not a regular
/// impl block, taking a list of field names and types similar to a struct
/// definition, instead of trait function implementations.
///
/// ### Example
///
/// ```
/// use engine::impl_game_object;
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
/// ```
///
/// For a more fully featured example for using these game objects, see the
/// documentation for [`Scene`](super::Scene).
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
        $crate::impl_game_object!(/push_info $infos/ $field_type);
        $crate::impl_game_object!(/push_info $infos/ $($field_types),+);
    };

    // Generators for the GameObject::components implementation
    (/push_component $components:ident, $self:ident/ $field_name:ident: $field_type:ty) => {
        $components.push((
            core::any::TypeId::of::<$field_type>(),
            bytemuck::cast_ref::<$field_type, [u8; size_of::<$field_type>()]>(&$self.$field_name),
        ));
    };
    (/push_component $components:ident, $self:ident/ $field_name:ident: $field_type:ty, $($field_names:ident: $field_types:ty),+) => {
        $crate::impl_game_object!(/push_component $components, $self/ $field_name: $field_type);
        $crate::impl_game_object!(/push_component $components, $self/ $($field_names: $field_types),+);
    };

    // The main impl-block generator
    (impl GameObject for $struct_name:ident using components {
        $($field_names:ident: $field_types:ty),+$(,)?
    }) => {
        impl $crate::game_objects::GameObject for $struct_name {
            fn component_infos(
            ) -> $crate::game_objects::ComponentVec<$crate::game_objects::ComponentInfo>
            {
                let mut infos = $crate::game_objects::ComponentVec::new();
                $crate::impl_game_object!(/push_info infos/ $($field_types),+);
                infos
            }

            fn components(
                &self,
            ) -> $crate::game_objects::ComponentVec<(core::any::TypeId, &[u8])>
            {
                let mut components: $crate::game_objects::ComponentVec::<
                    (core::any::TypeId, &[u8]),
                > = $crate::game_objects::ComponentVec::new();
                $crate::impl_game_object!(/push_component components, self/ $($field_names: $field_types),+);
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
        #[derive(Debug)]
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
