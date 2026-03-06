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

use crate::{impl_entity_id, AsAny, NodeManager};
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
    fn node_name(&self) -> &'static str;

    fn register_in(manager: &mut NodeManager) -> Result<(), String>
    where
        Self: Sized;
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
                    Ok(Self)
                }
            }

            impl $crate::NodeCore for $struct_name {
                fn node_name(&self) -> &'static str {
                    <$struct_name as $crate::NodeMeta>::NAME
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
