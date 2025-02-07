use cognet::{
    init_thread_pool, num_cpus,
    wasm_bindgen::{self, prelude::wasm_bindgen, JsValue},
    wasm_bindgen_futures,
};

#[wasm_bindgen(start)]
pub async fn main() -> Result<(), JsValue> {
    wasm_bindgen_futures::JsFuture::from(init_thread_pool(num_cpus::get()))
        .await
        .expect("Failed to initialize Rayon thread pool");

    Ok(())
}
