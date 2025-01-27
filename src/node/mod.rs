pub mod operators;
pub mod primitives;

pub use operators::*;
pub use primitives::*;

use crate::{AsAny, Data, EntityId, EvaluationContext, InputSlot, OutputSlot, SharedData};
use std::{
    any::Any,
    fmt::Debug,
    sync::{Arc, Mutex},
};

pub type NodeId = EntityId<Arc<dyn NodeImpl>>;

#[async_trait::async_trait]
pub trait NodeImpl: Debug + Send + Sync + NodeCore {
    fn initialize() -> Self
    where
        Self: Sized;

    async fn execute(
        &self,
        evaluation_context: Arc<Mutex<EvaluationContext>>,
    ) -> Result<(), String>;
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
        evaluation_context: Arc<Mutex<EvaluationContext>>,
        slot_index: usize,
    ) -> Result<Vec<SharedData>, String>;

    fn get_input_slot_by_index(&self, input_slot_index: usize) -> Option<&InputSlot>;

    fn get_output_slot_by_index(&self, output_slot_index: usize) -> Option<&OutputSlot>;

    fn get_input_slot_by_index_mut(&mut self, input_slot_index: usize) -> Option<&mut InputSlot>;

    fn set_output_value(
        &self,
        evaluation_context: Arc<Mutex<EvaluationContext>>,
        slot_index: usize,
        value: Data,
    ) -> Result<(), String>;

    fn get_output_slot_by_index_mut(&mut self, output_slot_index: usize)
        -> Option<&mut OutputSlot>;

    fn node_name(&self) -> &'static str;

    fn inputs(&self) -> &Vec<InputSlot>;

    fn inputs_mut(&mut self) -> &mut Vec<InputSlot>;

    fn outputs(&self) -> &Vec<OutputSlot>;

    fn outputs_mut(&mut self) -> &mut Vec<OutputSlot>;
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
            impl $crate::NodeCore for $struct_name {
                fn input_value(
                    &self,
                    evaluation_context: Arc<std::sync::Mutex<$crate::EvaluationContext>>,
                    slot_index: usize,
                ) ->  Result<Vec<$crate::SharedData>, String> {
                    let input_slot = self.inputs().get(slot_index).ok_or_else(|| "Invalid slot index".to_string())?;

                    let context = evaluation_context
                        .lock()
                        .map_err(|_| "Failed to lock context")?;
                    let result = input_slot.connected_edges.iter().filter_map(|edge_id| {
                        context.edges.get(edge_id).and_then(|edge| context.outputs.get(&edge.from_output_slot_id).map(Arc::clone))
                    }).collect();

                    Ok(result)
                }

                fn set_output_value(
                    &self,
                    evaluation_context: Arc<std::sync::Mutex<$crate::EvaluationContext>>,
                    slot_index: usize,
                    value: $crate::Data,
                ) -> Result<(), String> {
                    match self.outputs().get(slot_index) {
                        Some(slot) => match (&slot.data_type, &value) {
                            ($crate::DataType::Number, $crate::Data::Number(number)) => {
                                let mut context = evaluation_context.lock().map_err(|_| "Failed to lock context")?;
                                context
                                    .outputs
                                    .insert(slot.id.clone(), $crate::Data::Number(number.clone()).into());
                                Ok(())
                            }
                            ($crate::DataType::String, $crate::Data::String(string)) => {
                                let mut context = evaluation_context.lock().map_err(|_| "Failed to lock context")?;
                                context
                                    .outputs
                                    .insert(slot.id.clone(), $crate::Data::String(string.clone()).into());
                                Ok(())
                            }
                            (expected, actual) => Err(format!(
                                "Type mismatch: expected {:?}, but got {:?}",
                                expected, actual
                            )),
                        },
                        None => Err("Invalid value index".to_string()),
                    }
                }

                fn get_input_slot_by_index(
                    &self,
                    input_slot_index: usize,
                ) -> Option<&$crate::InputSlot> {
                    self.inputs().get(input_slot_index)
                }

                fn get_output_slot_by_index(
                    &self,
                    output_slot_index: usize,
                ) -> Option<&$crate::OutputSlot> {
                    self.outputs().get(output_slot_index)
                }

                fn get_input_slot_by_index_mut(
                    &mut self,
                    input_slot_index: usize,
                ) -> Option<&mut $crate::InputSlot> {
                    self.inputs.get_mut(input_slot_index)
                }

                fn get_output_slot_by_index_mut(
                    &mut self,
                    output_slot_index: usize,
                ) -> Option<&mut $crate::OutputSlot> {
                    self.outputs.get_mut(output_slot_index)
                }

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

pub trait NodePrimitive: NodeCore {
    fn set_default_value(
        &self,
        evaluation_context: Arc<Mutex<EvaluationContext>>,
        value: Data,
    ) -> Result<(), String> {
        self.set_output_value(evaluation_context, 0, value)
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
