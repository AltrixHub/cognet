pub mod operators;
pub mod primitives;

pub use operators::*;
pub use primitives::*;

use crate::{AsAny, Data, EntityId, InputSlot, OutputSlot, SharedExecutionCache};
use std::{any::Any, fmt::Debug, sync::Arc};

pub type NodeId = EntityId<Arc<dyn NodeImpl>>;

#[async_trait::async_trait]
pub trait NodeImpl: Debug + Send + Sync + NodeCore {
    fn initialize() -> Self
    where
        Self: Sized;

    async fn execute(&self, cache: SharedExecutionCache) -> Result<(), String>;
}

impl dyn NodeImpl {
    pub fn downcast_ref<T: NodeImpl + 'static>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }

    pub fn downcast_mut<T: NodeImpl + 'static>(&mut self) -> Option<&mut T> {
        self.as_any_mut().downcast_mut::<T>()
    }
}

pub trait NodeCore: Debug + Send + Sync + AsAny {
    fn input_value(
        &self,
        cache: SharedExecutionCache,
        slot_index: usize,
    ) -> Result<Vec<Data>, String> {
        let input_slot = self
            .inputs()
            .get(slot_index)
            .ok_or_else(|| "Invalid slot index".to_string())?;

        let cache = cache.lock()?;
        let result: Vec<Data> = input_slot
            .connected_edges
            .iter()
            .filter_map(|edge_id| {
                cache.edges.get(edge_id).and_then(|edge| {
                    cache
                        .outputs
                        .get(&edge.from_output_slot_id)
                        .and_then(|data| Some(data.share()))
                })
            })
            .collect();

        Ok(result)
    }

    fn get_input_slot_by_index(&self, input_slot_index: usize) -> Option<&InputSlot> {
        self.inputs().get(input_slot_index)
    }

    fn get_output_slot_by_index(&self, output_slot_index: usize) -> Option<&OutputSlot> {
        self.outputs().get(output_slot_index)
    }

    fn get_input_slot_by_index_mut(&mut self, input_slot_index: usize) -> Option<&mut InputSlot> {
        self.inputs_mut().get_mut(input_slot_index)
    }

    fn get_output_slot_by_index_mut(
        &mut self,
        output_slot_index: usize,
    ) -> Option<&mut OutputSlot> {
        self.outputs_mut().get_mut(output_slot_index)
    }

    fn node_name(&self) -> &'static str;

    fn inputs(&self) -> &Vec<InputSlot>;

    fn inputs_mut(&mut self) -> &mut Vec<InputSlot>;

    fn outputs(&self) -> &Vec<OutputSlot>;

    fn outputs_mut(&mut self) -> &mut Vec<OutputSlot>;
}

pub trait NodeDataAccess: NodeCore {
    fn set_output_value<T: Any + Send + Sync + Debug>(
        &self,
        cache: SharedExecutionCache,
        slot_index: usize,
        value: T,
    ) -> Result<(), String> {
        let new_data = Data::new::<T>(value)?;

        if let Some(slot) = self.outputs().get(slot_index) {
            match (slot.data_type, new_data.get_type()) {
                (expected, actual) if expected == actual => {
                    cache.lock()?.outputs.insert(slot.id.clone(), new_data);
                    Ok(())
                }
                (expected, actual) => Err(format!(
                    "Type mismatch: expected {:?}, but got {:?}",
                    expected, actual
                )),
            }
        } else {
            Err("Invalid value index".to_string())
        }
    }
}

impl<T: 'static + NodeCore> AsAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[macro_export]
macro_rules! impl_node_core {
    ($($struct_name:ident),*) => {
        $(
            impl $crate::NodeDataAccess for $struct_name {}

            impl $crate::NodeCore for $struct_name {
                fn node_name(&self) -> &'static str {
                    self.node_name
                }

                fn inputs(&self) -> &Vec<$crate::InputSlot> {
                    &self.inputs
                }

                fn inputs_mut(&mut self) -> &mut Vec<$crate::InputSlot> {
                    &mut self.inputs
                }

                fn outputs(&self) -> &Vec<$crate::OutputSlot> {
                    &self.outputs
                }

                fn outputs_mut(&mut self) -> &mut Vec<$crate::OutputSlot> {
                    &mut self.outputs
                }
            }
        )*
    };
}

pub trait NodePrimitive: NodeCore + NodeDataAccess {
    fn set_default_value<T: 'static + Send + Sync + Debug>(
        &self,
        cache: SharedExecutionCache,
        value: T,
    ) -> Result<(), String> {
        self.set_output_value(cache, 0, value)
    }
}

#[macro_export]
macro_rules! impl_primitive_node_core {
    ($($struct_name:ident),*) => {
        $(
            impl $crate::NodePrimitive for $struct_name {}

            impl_node_core!($struct_name);
        )*
    };
}
