pub mod execution_context;
pub mod operators;
pub mod outputs;
pub mod primitives;
pub mod subgraph;
pub mod type_info;

pub use execution_context::*;
pub use operators::*;
pub use outputs::*;
pub use primitives::*;
pub use subgraph::*;
pub use type_info::*;

use crate::{impl_entity_id, AsAny, Data, InputSlot, NodeManager, OutputSlot};
use std::{any::Any, fmt::Debug};

impl_entity_id!(NodeId);

/// Trait for node initialization.
/// This is automatically implemented by the `register_nodes!` macro.
pub trait NodeInit: NodeMeta + Sized {
    /// Initialize a new node instance.
    /// The default implementation creates a node with:
    /// - `node_data` from `NodeMeta::default_data()`
    /// - `inputs` from `NodeMeta::INPUTS`
    /// - `outputs` from `NodeMeta::OUTPUTS`
    fn initialize() -> Result<Self, String>;
}

/// The core node implementation trait.
/// Only requires `execute()` - initialization is handled by `NodeInit`.
#[async_trait::async_trait]
pub trait NodeImpl: Debug + Send + Sync + AsAny {
    async fn execute(&self, ctx: ExecutionContext) -> Result<(), String>;
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
            impl $crate::NodeInit for $struct_name {
                fn initialize() -> Result<Self, String> {
                    Ok(Self {
                        node_data: <Self as $crate::NodeMeta>::DEFAULT_VALUE.to_data(),
                        inputs: $crate::inputs_from_defs(<Self as $crate::NodeMeta>::INPUTS),
                        outputs: $crate::outputs_from_defs(<Self as $crate::NodeMeta>::OUTPUTS),
                    })
                }
            }

            impl $crate::NodeValueSetter for $struct_name {}

            impl $crate::NodeCore for $struct_name {
                fn node_name(&self) -> &'static str {
                    <$struct_name as $crate::NodeMeta>::NAME
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
                    let factory = std::sync::Arc::new(|| {
                        <$struct_name as $crate::NodeInit>::initialize().map(|node| {
                            std::sync::Arc::new(std::sync::RwLock::new(node)) as $crate::NodeEntity
                        })
                    });

                    // Register by TypeId for compile-time dispatch
                    manager.register_factory::<$struct_name>(factory.clone())?;

                    // Register by name for runtime dispatch
                    manager.register_factory_with_name(
                        <$struct_name as $crate::NodeMeta>::NAME,
                        factory,
                        <$struct_name as $crate::NodeMeta>::DEFAULT_VALUE.to_data(),
                    );

                    Ok(())
                }
            }

            // Register NodeTypeInfo for static metadata access
            inventory::submit! {
                $crate::NodeTypeInfo {
                    name: <$struct_name as $crate::NodeMeta>::NAME,
                    category: <$struct_name as $crate::NodeMeta>::CATEGORY,
                    inputs: <$struct_name as $crate::NodeMeta>::INPUTS,
                    outputs: <$struct_name as $crate::NodeMeta>::OUTPUTS,
                    default_value: <$struct_name as $crate::NodeMeta>::DEFAULT_VALUE,
                }
            }

            // Register factory for node creation
            inventory::submit! {
                $crate::NodeRegistrationEntry {
                    register: <$struct_name as $crate::NodeCore>::register_in,
                }
            }
        )*
    };
}
