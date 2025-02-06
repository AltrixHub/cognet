use cognet::init_thread_pool;
use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen(start)]
pub fn main() {
    init_thread_pool(4);
}
