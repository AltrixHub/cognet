pub mod operators;
pub mod primitives;

pub use operators::*;
pub use primitives::*;

use crate::{AsAny, Data, EvaluationContext, InputSlot, OutputSlot, TypedId};
use std::{any::Any, fmt::Debug, sync::Arc};

pub type NodeId = TypedId<Arc<dyn NodeImpl>>;

pub trait NodeImpl: Debug + Send + Sync + NodeCore {
    fn initialize() -> Self
    where
        Self: Sized;

    fn execute(&self, evaluation_context: &mut EvaluationContext) -> Result<(), String>;
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
    fn input_value<'a>(
        &'a self,
        evaluation_context: &'a EvaluationContext,
        slot_index: usize,
    ) -> Result<Vec<&'a Data>, String>;

    fn get_input_slot_by_index(&self, input_slot_index: usize) -> Option<&InputSlot>;

    fn get_output_slot_by_index(&self, output_slot_index: usize) -> Option<&OutputSlot>;

    fn get_input_slot_by_index_mut(&mut self, input_slot_index: usize) -> Option<&mut InputSlot>;

    fn set_output_value(
        &self,
        evaluation_context: &mut EvaluationContext,
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
                fn input_value<'a>(
                    &'a self,
                    evaluation_context: &'a $crate::EvaluationContext,
                    slot_index: usize,
                ) -> Result<Vec<&'a $crate::Data>, String> {
                    let mut result = Vec::new();
                    let input_slot = match self.inputs().get(slot_index) {
                        Some(slot) => slot,
                        None => return Err("Invalid slot index".to_string()),
                    };

                    for edge_id in &input_slot.connected_edges {
                        if let Some(edge) = evaluation_context.edges.get(edge_id) {
                            if let Some(value) =
                                evaluation_context.outputs.get(&edge.from_output_slot_id)
                            {
                                result.push(value);
                            }
                        }
                    }

                    Ok(result)
                }

                fn set_output_value(
                    &self,
                    evaluation_context: &mut $crate::EvaluationContext,
                    slot_index: usize,
                    value: $crate::Data,
                ) -> Result<(), String> {
                    match self.outputs().get(slot_index) {
                        Some(slot) => match (&slot.data_type, &value) {
                            ($crate::DataType::Number, $crate::Data::Number(number)) => {
                                evaluation_context
                                    .outputs
                                    .insert(slot.id.clone(), $crate::Data::Number(number.clone()));
                                Ok(())
                            }
                            ($crate::DataType::String, $crate::Data::String(string)) => {
                                evaluation_context
                                    .outputs
                                    .insert(slot.id.clone(), $crate::Data::String(string.clone()));
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
        evaluation_context: &mut EvaluationContext,
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
