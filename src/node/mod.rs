pub mod operators;
pub mod primitives;

pub use operators::*;
pub use primitives::*;

use crate::{
    impl_entity_id, AsAny, Data, InputSlot, NodeManager, OutputSlot, SharedExecutionCache,
};
use std::{any::Any, fmt::Debug};

impl_entity_id!(NodeId);

#[async_trait::async_trait]
pub trait NodeImpl: Debug + Send + Sync + AsAny {
    fn initialize() -> Result<Self, String>
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

pub trait NodeCore: Debug {
    fn input_value(
        &self,
        cache: SharedExecutionCache,
        slot_index: usize,
    ) -> Result<Vec<Data>, String> {
        let input_slot = self
            .inputs()
            .get(slot_index)
            .ok_or("Invalid slot index".to_string())?;

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

    fn node_data(&self) -> Option<Data>;

    fn node_data_mut(&mut self) -> &mut Option<Data>;

    fn inputs(&self) -> &Vec<InputSlot>;

    fn inputs_mut(&mut self) -> &mut Vec<InputSlot>;

    fn outputs(&self) -> &Vec<OutputSlot>;

    fn outputs_mut(&mut self) -> &mut Vec<OutputSlot>;

    fn register_in(manager: &mut NodeManager) -> Result<(), String>
    where
        Self: Sized;
}

pub trait NodeValueSetter: NodeCore {
    fn set_input_slot_default_data(&mut self, slot_index: usize, data: Data) -> Result<(), String> {
        let input_slot = self
            .inputs_mut()
            .get_mut(slot_index)
            .ok_or(format!("Invalid slot index: {}", slot_index))?;
        input_slot.set_default_value(data)?;
        Ok(())
    }

    fn set_node_data(&mut self, data: Data) -> Result<(), String> {
        *self.node_data_mut() = Some(data);
        Ok(())
    }

    fn set_output_data(
        &self,
        cache: SharedExecutionCache,
        slot_index: usize,
        data: Data,
    ) -> Result<(), String> {
        if let Some(slot) = self.outputs().get(slot_index) {
            if slot.data_type == data.get_type() {
                cache.lock()?.outputs.insert(slot.id, data);
                Ok(())
            } else {
                Err(format!(
                    "Data type mismatch: expected {:?}, but got {:?} in {}",
                    slot.data_type,
                    data.get_type(),
                    std::any::type_name::<Self>()
                ))
            }
        } else {
            Err(format!(
                "Index out of range: {} in {}",
                slot_index,
                std::any::type_name::<Self>()
            ))
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
macro_rules! register_nodes {
    ($($struct_name:ident),*) => {
        $(
            impl $crate::NodeValueSetter for $struct_name {}

            impl $crate::NodeCore for $struct_name {
                fn node_name(&self) -> &'static str {
                    self.node_name
                }

                fn node_data(&self) -> Option<$crate::Data> {
                    match &self.node_data {
                        Some(data) => Some(data.share()),
                        None => None
                    }
                }

                fn node_data_mut(&mut self) -> &mut Option<$crate::Data> {
                    &mut self.node_data
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

                fn register_in(manager: &mut $crate::NodeManager) -> Result<(), String> {
                    manager.register_factory::<$struct_name>(std::sync::Arc::new(|| {
                        $struct_name::initialize().map(|node| std::sync::Arc::new(tokio::sync::RwLock::new(node)) as $crate::NodeEntity)
                    }))
                }
            }

            inventory::submit! {
                $crate::NodeRegistrationEntry {
                    register: $struct_name::register_in,
                }
            }
        )*
    };
}
