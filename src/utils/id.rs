use std::{
    hash::{Hash, Hasher},
    marker::PhantomData,
};
use ulid::Ulid;

#[derive(Debug, Clone, Copy)]
pub struct EntityId<T: ?Sized> {
    id: Ulid,
    _marker: PhantomData<T>,
}

impl<T: ?Sized> Default for EntityId<T> {
    fn default() -> Self {
        Self {
            id: Ulid::new(),
            _marker: PhantomData,
        }
    }
}

impl<T: ?Sized> EntityId<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn as_ulid(&self) -> Ulid {
        self.id
    }
}

impl<T: ?Sized> PartialEq for EntityId<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T: ?Sized> Eq for EntityId<T> {}

impl<T: ?Sized> Hash for EntityId<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}
