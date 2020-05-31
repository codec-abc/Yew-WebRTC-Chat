#![recursion_limit = "128000"]

use wasm_bindgen::prelude::*;

mod chat;

use chat::chat_model::*;

#[allow(unused_imports)]
use yew::{html, App, Callback, Component, ComponentLink, Html, InputData, ShouldRender};

// Called when the wasm module is instantiated
#[wasm_bindgen(start)]
pub fn main() -> Result<(), JsValue> {
    // Use `web_sys`'s global `window` function to get a handle on the global
    // window object.
    let window = web_sys::window().expect("no global `window` exists");
    let document = window.document().expect("should have a document on window");
    //let body = document.body().expect("document should have a body");

    yew::initialize();
    let div = document.query_selector("#myRustApp").unwrap().unwrap();
    App::<ChatModel>::new().mount(div);
    yew::run_loop();

    Ok(())
}
