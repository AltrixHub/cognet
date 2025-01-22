use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use ulid::Ulid;

use crate::EdgeId;

#[derive(Debug, Clone, Copy, Default)]
pub struct TypedId<T: ?Sized> {
    id: Ulid,
    _marker: PhantomData<T>,
}

impl<T: ?Sized> TypedId<T> {
    pub fn new() -> Self {
        Self {
            id: Ulid::new(),
            _marker: PhantomData,
        }
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

pub type InputSlotId = TypedId<InputSlot>;
pub type OutputSlotId = TypedId<OutputSlot>;

pub enum SlotId {
    Input(InputSlotId),
    Output(OutputSlotId),
}

#[derive(Debug, PartialEq, Default, Clone)]
pub enum DataType {
    #[default]
    Number,
    String,
}

#[derive(Debug, Default, Clone)]
pub struct InputSlot {
    pub id: InputSlotId,
    pub label: &'static str,
    pub data_type: DataType,
    pub connected_edges: Vec<EdgeId>,
}

#[derive(Debug, Default, Clone)]
pub struct OutputSlot {
    pub id: OutputSlotId,
    pub label: &'static str,
    pub data_type: DataType,
    pub connected_edges: Vec<EdgeId>,
}
