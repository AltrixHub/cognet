use std::hash::Hash;
use ulid::Ulid;

pub trait EntityId: Clone + Copy + PartialEq + Eq + Hash + Default {
    fn new() -> Self;

    fn id(&self) -> Ulid;

    fn id_string(&self) -> String;
}

#[macro_export]
macro_rules! impl_entity_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name(ulid::Ulid);

        impl $name {
            pub fn new() -> Self {
                Self(ulid::Ulid::new())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl $crate::EntityId for $name {
            fn new() -> Self {
                Self::new()
            }

            fn id(&self) -> ulid::Ulid {
                self.0
            }

            fn id_string(&self) -> String {
                self.0.to_string()
            }
        }
    };
}
