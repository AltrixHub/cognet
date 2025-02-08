#![feature(unsafe_extern_blocks)]

use cognet::{
    init_thread_pool, num_cpus,
    wasm_bindgen::{self, prelude::wasm_bindgen, JsValue},
    wasm_bindgen_futures,
};
use web_sys::console;

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

    wasm_bindgen_futures::JsFuture::from(init_thread_pool(thread_num))
        .await
        .expect("Failed to initialize Rayon thread pool");

    Ok(())
}
