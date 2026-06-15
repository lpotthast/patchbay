#![recursion_limit = "256"]
#![cfg_attr(feature = "ssr", allow(dead_code))]

#[cfg(feature = "ssr")]
pub(crate) mod backend;
pub mod frontend;
pub mod shared;

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    leptos::mount::hydrate_body(frontend::App);
}
