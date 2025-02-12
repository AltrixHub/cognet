use crate::{Data, Edge, EdgeId, OutputSlotId};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard},
};

#[derive(Default)]
pub struct SharedExecutionCache {
    inner: Arc<Mutex<ExecutionCache>>,
}

impl SharedExecutionCache {
    pub fn new(cache: ExecutionCache) -> Self {
        SharedExecutionCache {
            inner: Arc::new(Mutex::new(cache)),
        }
    }

    pub fn lock(&self) -> Result<MutexGuard<ExecutionCache>, String> {
        Ok(self.inner.lock().map_err(|_| "Failed to lock cache")?)
    }

    pub fn share(&self) -> Self {
        SharedExecutionCache {
            inner: Arc::clone(&self.inner),
        }
    }
}

#[derive(Default, Debug)]
pub struct ExecutionCache {
    pub(crate) edges: HashMap<EdgeId, Edge>,
    pub(crate) outputs: HashMap<OutputSlotId, Data>,
}
