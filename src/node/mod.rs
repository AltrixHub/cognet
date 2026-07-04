pub mod execution_context;
pub mod interface;
pub mod operators;
pub mod outputs;
pub mod primitives;
pub mod subgraph;
pub mod type_info;

pub use execution_context::*;
pub use interface::*;
pub use operators::*;
pub use outputs::*;
pub use primitives::*;
pub use subgraph::*;
pub use type_info::*;

use crate::{impl_entity_id, AsAny, NodeManager};
use std::{any::Any, fmt::Debug, future::Future, pin::Pin};

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

/// Capability metadata describing how a node should be executed.
///
/// The graph executor inspects this to dispatch CPU-bound work synchronously
/// (potentially in parallel via rayon) and I/O-bound work asynchronously.
/// `SyncCpu` nodes implement [`NodeImpl::execute_sync`]; `AsyncIo` nodes
/// implement [`NodeImpl::prepare_async`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeExecutionKind {
    /// CPU-bound, deterministic, runs to completion without yielding to an
    /// async runtime. Default for all primitive and operator nodes.
    #[default]
    SyncCpu,
    /// I/O-bound. Must yield to an async runtime via the future returned
    /// from `prepare_async`.
    AsyncIo,
}

/// `'static` boxed future returned by [`NodeImpl::prepare_async`].
///
/// The future must own (or `Arc`-clone) every input it needs — the graph
/// executor drops all node and graph locks before awaiting it. This is the
/// invariant that prevents nested `block_on` and lock-across-`await`
/// deadlocks.
pub type BoxNodeFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'static>>;

/// The core node implementation trait.
///
/// Nodes split into two camps via [`NodeExecutionKind`]:
///
/// - **`SyncCpu`** (default) — implement [`NodeImpl::execute_sync`]. The
///   executor runs them on the current thread (or a rayon worker) inside
///   `NodeGraph::execute_sync`. They never enter an async runtime.
/// - **`AsyncIo`** — implement [`NodeImpl::prepare_async`]. The executor
///   calls `prepare_async` under a short read lock to obtain a `'static`
///   future, drops the lock, then awaits the future. Use for remote API
///   calls, asset loading, database access — anything that yields.
///
/// `NodeGraph::execute_sync` returns
/// [`crate::GraphExecutionError::RequiresAsyncExecution`] if the dirty
/// plan reaches an `AsyncIo` node. Use `NodeGraph::execute_async` for
/// mixed graphs.
pub trait NodeImpl: Debug + Send + Sync + AsAny {
    /// How the executor should dispatch this node.
    ///
    /// Default is [`NodeExecutionKind::SyncCpu`]. Override and implement
    /// [`NodeImpl::prepare_async`] for I/O-bound nodes.
    fn execution_kind(&self) -> NodeExecutionKind {
        NodeExecutionKind::SyncCpu
    }

    /// Synchronous execution entry point for `SyncCpu` nodes.
    ///
    /// Default implementation returns an error so a forgotten override
    /// surfaces as a runtime failure rather than silently doing nothing.
    fn execute_sync(&self, _ctx: ExecutionContext) -> Result<(), String> {
        Err(format!(
            "node `{}` does not implement synchronous execution; \
             override `NodeImpl::execute_sync` or set \
             `execution_kind() == NodeExecutionKind::AsyncIo` and implement `prepare_async`",
            std::any::type_name::<Self>()
        ))
    }

    /// Async preparation entry point for `AsyncIo` nodes.
    ///
    /// The implementation is called by the executor under a short read
    /// lock on the node entity. It must extract every input it needs from
    /// `ctx`, then return a `'static` future that owns its captured data.
    /// The executor drops all locks before awaiting that future, which is
    /// the invariant that keeps the executor free of nested `block_on`
    /// and `RwLock` guards across `.await`.
    fn prepare_async(&self, _ctx: ExecutionContext) -> Result<BoxNodeFuture, String> {
        Err(format!(
            "node `{}` does not implement asynchronous execution; \
             override `NodeImpl::prepare_async` or set \
             `execution_kind() == NodeExecutionKind::SyncCpu` and implement `execute_sync`",
            std::any::type_name::<Self>()
        ))
    }
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
                        Some(std::any::TypeId::of::<$struct_name>()),
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
