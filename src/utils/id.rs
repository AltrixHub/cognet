use std::{
    hash::{Hash, Hasher},
    marker::PhantomData,
};
use ulid::Ulid;

#[derive(Debug, Clone, Copy)]
pub struct TypedId<T: ?Sized> {
    id: Ulid,
    _marker: PhantomData<T>,
}

impl<T: ?Sized> Default for TypedId<T> {
    fn default() -> Self {
        Self {
            id: Ulid::new(),
            _marker: PhantomData,
        }
    }
}

impl<T: ?Sized> TypedId<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn as_ulid(&self) -> Ulid {
        self.id
    }
}

impl<T: ?Sized> PartialEq for TypedId<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T: ?Sized> Eq for TypedId<T> {}

impl<T: ?Sized> Hash for TypedId<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}
