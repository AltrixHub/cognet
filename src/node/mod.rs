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

use crate::{impl_entity_id, AsAny, Data, NodeManager};
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

/// Whether a node takes part in **value-based execution cutoff**.
///
/// The executor's dirty plan is pure reachability: everything downstream
/// of a changed node is replanned. Reachability cannot know that a node
/// re-ran and produced the *same* values as last time, so an unchanged
/// result still drags its whole downstream cone through a re-execution.
///
/// A node that opts in lets the executor answer that question. After the
/// node runs, the executor hands it the outputs cached BEFORE the run and
/// the ones now in the cache, and asks
/// [`NodeImpl::outputs_equivalent`]. When the answer is "equivalent", the
/// node is not recorded as changed, and every planned node whose inputs
/// all come from unchanged nodes is skipped for this pass — not executed,
/// absent from [`crate::ExecutionResult::node_outputs`], and left with its
/// cached outputs (still valid) and its clean dirty state intact.
///
/// # Default is off — on purpose
///
/// [`OutputCutoff::Disabled`] is the default, and it reproduces the
/// reachability-only behaviour exactly: an executed node always counts as
/// changed. Opting in is a per-node decision because the equality test is
/// only worth its cost where a node is a genuine value boundary (a
/// fan-in whose result is stable for most inputs, a filter that forwards
/// its input untouched). Forcing a deep comparison of large payloads on
/// every node would cost more than the re-execution it saves.
///
/// # What an opt-in promises
///
/// - **True equivalence.** `outputs_equivalent` must be reflexive,
///   symmetric and transitive, and "equivalent" must mean *no downstream
///   node can observe a difference*. Anything weaker (a tolerance, a
///   subset comparison, a hash with collisions) turns into stale
///   downstream values that no later execution will repair, because the
///   skipped nodes are never marked dirty again.
/// - **The outputs are the whole story.** Only nodes whose output slots
///   fully determine what downstream sees may opt in. A node that
///   forwards values by another route — notably
///   [`crate::InterfaceNode`], whose consumers resolve through it rather
///   than from its (never written) cache — must stay `Disabled`.
/// - **Cheap.** The comparison runs on every execution of the node, so it
///   must be cheaper than the downstream cone it can prune.
///
/// # Fail-open
///
/// Everything the executor cannot decide runs the node: a directly-dirty
/// node (structural change, node-data update, edge change) always
/// executes, a node with any changed upstream always executes, a node
/// whose upstream cannot be resolved always executes, and a node that
/// FAILED counts as changed so its downstream re-runs. Cutoff can only
/// ever remove work that provably has no observable effect.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutputCutoff {
    /// Executing this node always counts as "outputs changed"; its
    /// downstream cone always re-runs. Default — identical to the
    /// behaviour before value-based cutoff existed.
    #[default]
    Disabled,
    /// After each run the executor calls
    /// [`NodeImpl::outputs_equivalent`] with the previous and current
    /// outputs; an equivalent result stops propagation at this node.
    Enabled,
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

    /// Whether this node takes part in value-based execution cutoff.
    ///
    /// Default is [`OutputCutoff::Disabled`] — executing the node always
    /// counts as a change. Override to [`OutputCutoff::Enabled`] *and*
    /// implement [`NodeImpl::outputs_equivalent`] to let an unchanged
    /// result prune the downstream cone. Read the [`OutputCutoff`] docs
    /// before opting in: an equality that is not a true equivalence
    /// leaves permanently stale downstream values.
    fn output_cutoff(&self) -> OutputCutoff {
        OutputCutoff::Disabled
    }

    /// Are the outputs this node just produced equivalent to the ones it
    /// had before the run?
    ///
    /// Called only when [`NodeImpl::output_cutoff`] is
    /// [`OutputCutoff::Enabled`]. Both slices are indexed by output slot
    /// index and are the same length as the node's output slot list; a
    /// slot with no cached value (never written, or evicted) is `None`.
    ///
    /// Returning `true` means "no downstream node can observe a
    /// difference" and lets the executor skip this node's downstream cone.
    /// Returning `false` is always safe — it is the default behaviour.
    fn outputs_equivalent(&self, _previous: &[Option<Data>], _current: &[Option<Data>]) -> bool {
        false
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
