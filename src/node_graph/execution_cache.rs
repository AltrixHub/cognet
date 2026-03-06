use crate::{Data, DataType, OutputSlotId};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

#[derive(Default, Clone)]
pub struct SharedExecutionCache {
    inner: Arc<RwLock<ExecutionCache>>,
}

impl SharedExecutionCache {
    pub fn new(cache: ExecutionCache) -> Self {
        SharedExecutionCache {
            inner: Arc::new(RwLock::new(cache)),
        }
    }

    /// Acquire a write lock (backward-compatible with existing callers).
    pub fn lock(&self) -> Result<RwLockWriteGuard<'_, ExecutionCache>, String> {
        self.inner
            .write()
            .map_err(|_| "Failed to lock cache".to_string())
    }

    /// Acquire a read lock for read-only access.
    pub fn read(&self) -> Result<RwLockReadGuard<'_, ExecutionCache>, String> {
        self.inner
            .read()
            .map_err(|_| "Failed to read cache".to_string())
    }

    pub fn share(&self) -> Self {
        SharedExecutionCache {
            inner: Arc::clone(&self.inner),
        }
    }
}

/// Cache for computed output values.
///
/// Edge topology has been moved to `NodeStates`. This cache only stores
/// output data produced during graph execution.
#[derive(Default, Debug)]
pub struct ExecutionCache {
    pub(crate) outputs: HashMap<OutputSlotId, Data>,
}

use std::collections::HashMap;

impl ExecutionCache {
    /// Get output value by slot ID.
    pub fn get_output(&self, output_slot_id: &OutputSlotId) -> Option<&Data> {
        self.outputs.get(output_slot_id)
    }

    /// Remove all `DataType::Mesh` outputs from the cache.
    ///
    /// Frees `Arc<dyn Any>` payloads (typically containing mesh vertex/index data)
    /// while preserving Number and String outputs needed for edge value display
    /// and incremental re-execution.
    ///
    /// Call this after the application layer has consumed mesh data (e.g., uploaded
    /// to GPU) and no longer needs the cached copies.
    ///
    /// **Note:** This evicts ALL `DataType::Mesh` outputs regardless of the
    /// concrete type stored inside. Use [`evict_mesh_outputs_of_type`] to evict
    /// only outputs of a specific concrete type while keeping others.
    pub fn evict_mesh_outputs(&mut self) -> usize {
        let before = self.outputs.len();
        self.outputs.retain(|_, data| data.get_type() != DataType::Mesh);
        before - self.outputs.len()
    }

    /// Remove `DataType::Mesh` outputs that contain a specific concrete type `T`.
    ///
    /// Unlike [`evict_mesh_outputs`], this only evicts Mesh outputs whose
    /// `Arc<dyn Any>` payload can be downcast to `T`. Other Mesh-typed outputs
    /// (e.g., intermediate data like `WindowDef` or `WallDefinition`) are preserved.
    pub fn evict_mesh_outputs_of_type<T: 'static>(&mut self) -> usize {
        let before = self.outputs.len();
        self.outputs.retain(|_, data| {
            if data.get_type() != DataType::Mesh {
                return true; // keep non-mesh outputs
            }
            // Only evict if the concrete type matches T
            data.value::<T>().is_err()
        });
        before - self.outputs.len()
    }
}
