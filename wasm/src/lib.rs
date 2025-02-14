#![feature(unsafe_extern_blocks)]

use std::{cell::RefCell, rc::Rc};

use cognet::{Data, EntityId, NodeGraph, NodeGraphAPI, NodeId};
use wasm_bindgen::{prelude::wasm_bindgen, JsValue};
use wasm_bindgen_futures::{self, future_to_promise};
use wasm_bindgen_rayon::init_thread_pool;
use web_sys::{console, js_sys::Promise};

#[cfg(target_family = "wasm")]
unsafe extern "C" {
    fn __wasm_call_ctors();
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen(start)]
pub async fn start() -> Result<(), JsValue> {
    unsafe {
        __wasm_call_ctors();
    }

    let thread_num = num_cpus::get();
    console::log_1(&JsValue::from_str(&format!(
        "Initializing thread pool... {:?}",
        thread_num
    )));

    Ok(())
}

#[wasm_bindgen]
pub struct WasmNodeGraph {
    inner: Rc<RefCell<NodeGraph>>,
}

#[wasm_bindgen]
impl WasmNodeGraph {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<WasmNodeGraph, JsValue> {
        NodeGraph::new()
            .map(|graph| WasmNodeGraph {
                inner: Rc::new(RefCell::new(graph)),
            })
            .map_err(|e| JsValue::from_str(&e))
    }

    #[wasm_bindgen]
    pub fn execute(&self) -> Promise {
        let inner = Rc::clone(&self.inner);
        let fut = async move {
            let mut graph = inner.borrow_mut();
            graph
                .execute()
                .await
                .map(|_| JsValue::undefined())
                .map_err(|e| JsValue::from_str(&e))
        };
        future_to_promise(fut)
    }

    #[wasm_bindgen]
    pub fn remove_node(&self, node_id: String) -> Promise {
        let inner = Rc::clone(&self.inner);
        let fut = async move {
            let mut graph = inner.borrow_mut();
            let id = NodeId::from_string(&node_id)?;
            graph
                .remove_node(id)
                .await
                .map(|_| JsValue::undefined())
                .map_err(|e| JsValue::from_str(&e))
        };
        future_to_promise(fut)
    }

    #[wasm_bindgen]
    pub fn update_node_data(&self, node_id: String, data: JsValue) -> Promise {
        let inner = Rc::clone(&self.inner);
        let fut = async move {
            let mut graph = inner.borrow_mut();
            let id = NodeId::from_string(&node_id)?;
            let data: Data = serde_wasm_bindgen::from_value(data).map_err(|err| err.to_string())?;
            graph
                .update_node_data(&id, data)
                .await
                .map(|_| JsValue::undefined())
                .map_err(|e| JsValue::from_str(&e))
        };
        future_to_promise(fut)
    }

    #[wasm_bindgen]
    pub fn update_input_slot_default_data(
        &self,
        node_id: String,
        slot_index: usize,
        data: JsValue,
    ) -> Promise {
        let inner = Rc::clone(&self.inner);
        let fut = async move {
            let mut graph = inner.borrow_mut();
            let id = NodeId::from_string(&node_id)?;
            let data: Data = serde_wasm_bindgen::from_value(data).map_err(|err| err.to_string())?;
            graph
                .update_input_slot_default_data(&id, slot_index, data)
                .await
                .map(|_| JsValue::undefined())
                .map_err(|e| JsValue::from_str(&e))
        };
        future_to_promise(fut)
    }

    #[wasm_bindgen]
    pub fn connect_nodes(
        &self,
        from_node_id: String,
        from_output_slot_index: usize,
        to_node_id: String,
        to_input_slot_index: usize,
    ) -> Promise {
        let inner = Rc::clone(&self.inner);
        let fut = async move {
            let mut graph = inner.borrow_mut();
            let from_id = NodeId::from_string(&from_node_id)?;
            let to_id = NodeId::from_string(&to_node_id)?;
            graph
                .connect_nodes(
                    &from_id,
                    from_output_slot_index,
                    &to_id,
                    to_input_slot_index,
                )
                .await
                .map(|edge_id| JsValue::from(edge_id.id_string()))
                .map_err(|e| JsValue::from_str(&e))
        };
        future_to_promise(fut)
    }

    #[wasm_bindgen]
    pub fn create_node(&self) -> Promise {
        let inner = Rc::clone(&self.inner);
        let fut = async move {
            let mut graph = inner.borrow_mut();
        };
        unimplemented!();
        // future_to_promise(fut)
    }

    // #[wasm_bindgen]
    // pub fn remove_edge(&self, edge_id: u32) -> Promise {
    //     let inner = self.inner.clone();
    //     let fut = async move {
    //         let mut graph = inner.borrow_mut();
    //         let id = EdgeId::from(edge_id);
    //         graph
    //             .remove_edge(id)
    //             .await
    //             .map(|_| JsValue::undefined())
    //             .map_err(|e| JsValue::from_str(&e))
    //     };
    //     future_to_promise(fut)
    // }

    // #[wasm_bindgen]
    // pub fn get_edge(&self, edge_id: u32) -> JsValue {
    //     let graph = self.inner.borrow();
    //     let id = EdgeId::from(edge_id);
    //     match graph.get_edge(id) {
    //         Ok(edge) => JsValue::from_serde(&edge).unwrap_or(JsValue::NULL),
    //         Err(e) => JsValue::from_str(&e),
    //     }
    // }

    // #[wasm_bindgen]
    // pub fn get_node_by_id(&self, node_id: String) -> Promise {
    //     let inner = self.inner.clone();
    //     let fut = async move {
    //         let graph = inner.borrow();
    //         let id = NodeId::from_string(&node_id)?;
    //         match graph.get_node_by_id(&id).await {
    //             Some(node) => {
    //                 JsValue::from_serde(&node).map_err(|e| JsValue::from_str(&e.to_string()))
    //             }
    //             None => Ok(JsValue::NULL),
    //         }
    //     };
    //     future_to_promise(fut)
    // }

    // #[wasm_bindgen]
    // pub fn get_nodes_by_ids(&self, ids: JsValue) -> Promise {
    //     let inner = self.inner.clone();
    //     let fut = async move {
    //         let graph = inner.borrow();
    //         let id_vec: Vec<u32> = serde_wasm_bindgen::from_value(ids)
    //             .map_err(|err| JsValue::from_str(&err.to_string()))?;
    //         let node_ids = id_vec.into_iter().map(NodeId::from).collect();
    //         let nodes = graph.get_nodes_by_ids(node_ids).await;
    //         JsValue::from_serde(&nodes).map_err(|e| JsValue::from_str(&e.to_string()))
    //     };
    //     future_to_promise(fut)
    // }

    // #[wasm_bindgen]
    // pub fn get_output_value(&self, node_id: String, output_slot_index: usize) -> Promise {
    //     let inner = self.inner.clone();
    //     let fut = async move {
    //         let graph = inner.borrow();
    //         let id = NodeId::from_string(&node_id)?;
    //         match graph.get_output_value(&id, output_slot_index).await {
    //             Some(data) => {
    //                 JsValue::from_serde(&data).map_err(|e| JsValue::from_str(&e.to_string()))
    //             }
    //             None => Ok(JsValue::NULL),
    //         }
    //     };
    //     future_to_promise(fut)
    // }
}
