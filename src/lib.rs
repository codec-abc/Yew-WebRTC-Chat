#![recursion_limit = "128000"]

use wasm_bindgen::prelude::*;

mod chat;

use chat::chat_model::*;

use crate::chat::web_rtc_manager::WebRTCManager;

// Called when the wasm module is instantiated
#[wasm_bindgen(start)]
pub fn main() -> Result<(), JsValue> {
    yew::Renderer::<ChatModel<WebRTCManager>>::new().render();
    Ok(())
}
