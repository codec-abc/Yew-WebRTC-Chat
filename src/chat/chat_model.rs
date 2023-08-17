use std::cell::RefCell;
use std::rc::Rc;
use std::str;

use base64;
use serde::{Deserialize, Serialize};

use web_sys::{console, Element, HtmlInputElement, InputEvent, RtcDataChannelState};

use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use yew::{html, html::NodeRef, Context, Component, Html, KeyboardEvent, TargetCast};

use crate::chat::web_rtc_manager::{ConnectionState, IceCandidate, NetworkManager, State};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MessageSender {
    Me,
    Other,
}

#[derive(Clone, Debug)]
pub struct Message {
    sender: MessageSender,
    content: String,
}

impl Message {
    pub fn new(content: String, sender: MessageSender) -> Message {
        Message { content, sender }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ConnectionString {
    pub ice_candidates: Vec<IceCandidate>,
    pub offer: String, // TODO : convert as JsValue using Json.Parse
}
pub struct ChatModel<T: NetworkManager + 'static> {
    web_rtc_manager: Rc<RefCell<T>>,
    messages: Vec<Message>,
    value: String,
    chat_value: String,
    node_ref: NodeRef,
}

#[derive(Clone, Debug)]
pub enum Msg {
    StartAsServer,
    ConnectToServer,
    UpdateWebRTCState(State),
    Disconnect,
    Send,
    NewMessage(Message),
    UpdateInputValue(String),
    UpdateInputChatValue(String),
    OnKeyUp(KeyboardEvent),
    CopyToClipboard,
    ValidateOffer,
    ResetWebRTC,
}

// UI done from: https://codepen.io/sajadhsm/pen/odaBdd

impl<T: NetworkManager + 'static> Component for ChatModel<T> {
    type Message = Msg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        ChatModel {
            web_rtc_manager: T::new(ctx.link()),
            messages: vec![],
            value: "".into(),
            chat_value: "".into(),
            node_ref: NodeRef::default(),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
    // fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::StartAsServer => {
                self.web_rtc_manager
                    .borrow_mut()
                    .set_state(State::Server(ConnectionState::new()));
                T::start_web_rtc(self.web_rtc_manager.clone())
                    .expect("Failed to start WebRTC manager");

                true
            }

            Msg::ConnectToServer => {
                self.web_rtc_manager
                    .borrow_mut()
                    .set_state(State::Client(ConnectionState::new()));
                T::start_web_rtc(self.web_rtc_manager.clone())
                    .expect("Failed to start WebRTC manager");

                true
            }

            Msg::UpdateWebRTCState(web_rtc_state) => {
                self.value = "".into();
                let debug = get_debug_state_string(&web_rtc_state);
                console::log_1(&debug.into());

                // let debug = self.get_serialized_offer_and_candidates();
                // let hash = hmac_sha256::Hash::hash(debug.as_bytes());
                // let hash_as_string = hex::encode(hash);
                // console::log_1(&hash_as_string.into());

                true
            }

            Msg::ResetWebRTC => {
                self.web_rtc_manager = T::new(ctx.link());
                self.messages = vec![];
                self.chat_value = "".into();
                self.value = "".into();

                true
            }

            Msg::UpdateInputValue(val) => {
                self.value = val;

                true
            }

            Msg::UpdateInputChatValue(val) => {
                self.chat_value = val;

                true
            }

            Msg::ValidateOffer => {
                let state = self.web_rtc_manager.borrow().get_state();

                match state {
                    State::Server(_connection_state) => {
                        let result = T::validate_answer(self.web_rtc_manager.clone(), &self.value);

                        if result.is_err() {
                            web_sys::Window::alert_with_message(
                                &web_sys::window().unwrap(),
                                &format!(
                                    "Cannot use answer. Failure reason: {:?}",
                                    result.err().unwrap()
                                ),
                            )
                            .expect("alert should work");
                        }
                    }
                    _ => {
                        let result = T::validate_offer(self.web_rtc_manager.clone(), &self.value);

                        if result.is_err() {
                            web_sys::Window::alert_with_message(
                                &web_sys::window().unwrap(),
                                &format!(
                                    "Cannot use offer. Failure reason: {:?}",
                                    result.err().unwrap()
                                ),
                            )
                            .expect("alert should work");
                        }
                    }
                };

                true
            }

            Msg::NewMessage(message) => {
                self.messages.push(message);
                self.scroll_top();

                true
            }

            Msg::Send => {
                let my_message = Message::new(self.chat_value.clone(), MessageSender::Me);
                self.messages.push(my_message);
                self.web_rtc_manager.borrow().send_message(&self.chat_value);
                self.chat_value = "".into();
                self.scroll_top();

                true
            }

            Msg::Disconnect => {
                self.web_rtc_manager = T::new(ctx.link());
                self.messages = vec![];
                self.chat_value = "".into();
                self.value = "".into();

                true
            }

            Msg::OnKeyUp(event) => {
                if event.key_code() == 13 && !self.chat_value.is_empty() {
                    let my_message = Message::new(self.chat_value.clone(), MessageSender::Me);
                    self.messages.push(my_message);
                    self.web_rtc_manager.borrow().send_message(&self.chat_value);
                    self.chat_value = "".into();
                    self.scroll_top();
                }

                true
            }

            Msg::CopyToClipboard => {
                self.copy_content_to_clipboard();

                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let content = match &self.web_rtc_manager.borrow().get_state() {
            State::Default => {
                html! {
                    <>
                        { self.get_chat_header(ctx) }

                        <main class="msger-chat" id="chat-main" ref={self.node_ref.clone()}>
                            <div class="msg left-msg">
                                <div class="msg-bubble">

                                    <div class="msg-text">
                                        {"Hi, welcome to SimpleChat!
                                        To start you need to establish connection with your friend. Either click the button below to start generate an offer and create a code to send to your friend."}
                                        <br/>
                                        <button
                                            class="msger-send-btn"
                                            style="border-radius: 3px; padding: 10px; font-size: 1em; border: none; margin-left: 0px; margin-top: 6px;"
                                            onclick={ctx.link().callback(move |_| Msg::StartAsServer)}>
                                            {"I will generate an offer first!"}
                                        </button>
                                    </div>
                                </div>
                            </div>

                            <div class="msg right-msg">

                                <div class="msg-bubble">

                                    <div class="msg-text">
                                        {"Alternatively, if your friend has already a code click the button below."}
                                        <br/>
                                            <button
                                                class="msger-send-btn"
                                                style="border-radius: 3px; padding: 10px; font-size: 1em; border: none; margin-left: 0px; margin-top: 6px; float: right;"
                                                onclick={ctx.link().callback(move |_| Msg::ConnectToServer)}>
                                                {"My friend already send me a code!"}
                                            </button>
                                    </div>
                                </div>
                            </div>
                        </main>

                        { self.get_input_for_chat_message(ctx) }
                    </>
                }
            }

            State::Server(connection_state) => {
                html! {
                    <>
                        { self.get_chat_header(ctx) }

                        <main class="msger-chat" id="chat-main" ref={self.node_ref.clone()}>
                        {
                            if
                                connection_state.data_channel_state.is_some() &&
                                connection_state.data_channel_state.unwrap() == RtcDataChannelState::Open
                            {
                                html! {

                                    <>
                                        { self.get_messages_as_html() }
                                    </>
                                }
                            } else if connection_state.ice_gathering_state.is_some() {
                                html! {
                                    <>

                                        <div class="msg left-msg">

                                            <div class="msg-bubble">
                                                <div class="msg-info">
                                                </div>

                                                <div class="msg-text">
                                                    { self.get_offer_and_candidates(ctx) }
                                                </div>
                                            </div>
                                        </div>

                                        <div class="msg left-msg">

                                            <div class="msg-bubble">
                                                <div class="msg-info">
                                                </div>

                                                <div class="msg-text">
                                                    { "And then paste his/her answer below "}
                                                    { self.get_validate_offer_or_answer(ctx) }
                                                </div>
                                            </div>
                                        </div>
                                    </>
                                }
                            } else {
                                html! {}
                            }
                        }
                        </main>

                        { self.get_input_for_chat_message(ctx) }
                    </>
                }
            }

            State::Client(connection_state) => {
                html! {
                    <>
                        { self.get_chat_header(ctx) }

                        <main class="msger-chat" id="chat-main" ref={self.node_ref.clone()}>
                        {

                            if connection_state.data_channel_state.is_some()
                                && connection_state.data_channel_state.unwrap() == RtcDataChannelState::Open
                            {
                                html! {
                                    <>
                                        { self.get_messages_as_html() }
                                    </>
                                }
                            } else if connection_state.ice_gathering_state.is_some() {
                                html! {

                                    <div class="msg right-msg">

                                        <div class="msg-bubble">
                                            <div class="msg-info">
                                            </div>

                                            <div class="msg-text">
                                                { self.get_offer_and_candidates(ctx) }
                                            </div>
                                        </div>
                                    </div>

                                }
                            } else {
                                html! {

                                <>
                                    <div class="msg right-msg">

                                        <div class="msg-bubble">
                                            <div class="msg-info">
                                            </div>

                                            <div class="msg-text">
                                                { "Paste here the offer given by your friend:" }
                                                { self.get_validate_offer_or_answer(ctx) }
                                            </div>
                                        </div>
                                    </div>

                                    <div class="msg right-msg">

                                        <div class="msg-bubble">
                                            <div class="msg-info">
                                            </div>

                                            <div class="msg-text">
                                                { "If after a while the connection cannot be establish, it is probably because there is a network issue between the 2 computers." }
                                            </div>
                                        </div>
                                    </div>
                                </>
                                }
                            }
                        }

                        </main>

                        { self.get_input_for_chat_message(ctx) }
                    </>
                }
            }
        };

        html! {
            <div class="msger">
                { content }
            </div>
        }
    }
}

impl<T: NetworkManager + 'static> ChatModel<T> {
    fn scroll_top(&self) {
        let node_ref = self.node_ref.clone();

        spawn_local(async move {
            let chat_main = node_ref.cast::<Element>().unwrap();
            let current_scroll_top = chat_main.scroll_top();
            chat_main.set_scroll_top(current_scroll_top + 100000000);
        })
    }

    fn get_chat_header(&self, ctx: &Context<Self>) -> Html {
        let is_disconnect_button_visible =
            self.web_rtc_manager.borrow().get_state() != State::Default;
        html! {
            <header class="msger-header">
                <div style="font-size:25">
                    {"Rust WebRTC WASM Chat V2.3"}
                </div>

                { self.get_debug_html() }

                {
                    if is_disconnect_button_visible {
                        html! {
                            <div>
                                <button
                                    style="background: red; border-radius: 3px; padding: 6px; font-size: 1em; border: none; margin-left: 0px; margin-top: 0px; float: right;"
                                    class="msger-send-btn"
                                    onclick={ctx.link().callback(move |_| Msg::Disconnect)}>
                                    {"Disconnect"}
                                </button>
                            </div>
                        }
                    } else {
                        html! {<> </>}
                    }
                }
            </header>
        }
    }

    fn is_chat_enabled(&self) -> bool {
        match &self.web_rtc_manager.borrow().get_state() {
            State::Default => false,
            State::Server(connection_state) => {
                connection_state.data_channel_state.is_some()
                    && connection_state.data_channel_state.unwrap() == RtcDataChannelState::Open
            }
            State::Client(connection_state) => {
                connection_state.data_channel_state.is_some()
                    && connection_state.data_channel_state.unwrap() == RtcDataChannelState::Open
            }
        }
    }

    fn get_input_for_chat_message(&self, ctx: &Context<Self>) -> Html {
        let is_chat_enabled = self.is_chat_enabled();
        let is_send_button_enabled = is_chat_enabled && !self.chat_value.is_empty();

        html! {
            <div>
                <div
                    class="msger-inputarea"
                    disabled={!is_chat_enabled}
                >
                    <input
                        type="text"
                        class="msger-input"
                        disabled={!is_chat_enabled}
                        id="chat-message-box"
                        value={self.chat_value.clone()}
                        oninput={ctx.link().callback(|e: InputEvent| {Msg::UpdateInputChatValue(e.target_unchecked_into::<HtmlInputElement>().value())})}
                        onkeyup={ctx.link().callback(move |e: KeyboardEvent| Msg::OnKeyUp(e))}
                    />
                    <button
                        class="msger-send-btn"
                        disabled={!is_send_button_enabled}
                        onclick={ctx.link().callback(move |_| Msg::Send)}
                    >
                        {"Send"}
                    </button>
                </div>
            </div>
        }
    }

    fn get_validate_offer_or_answer(&self, ctx: &Context<Self>) -> Html {
        html! {
            <>
            <div
                style="padding:0;"
                class="msger-inputarea"
            >
                <textarea
                    class="msger-input"
                    style="background:white;"
                    value={self.value.clone()}
                    oninput={ctx.link().callback(|e: InputEvent| {Msg::UpdateInputValue(e.target_unchecked_into::<HtmlInputElement>().value())})}
                >
                </textarea >
            </div>

                <button
                    class="msger-send-btn"
                    style="border-radius: 3px; padding: 10px; font-size: 1em; border: none; margin-left: 0px; margin-top: 6px; float: right;"
                    onclick={ctx.link().callback(move |_| Msg::ValidateOffer)}
                >
                    {"Validate Offer"}

                    //TODO once clicked the message should disappear and say that if connection cannot be established after a while
                    // there is probably a network issue
                </button>
            </>
        }
    }

    fn get_serialized_offer_and_candidates(&self) -> String {
        let connection_string = ConnectionString {
            offer: self
                .web_rtc_manager
                .borrow()
                .get_offer()
                .expect("no offer yet"),
            ice_candidates: self.web_rtc_manager.borrow().get_ice_candidates(),
        };

        let serialized: String = serde_json::to_string(&connection_string).unwrap();

        base64::encode(serialized)
    }

    fn get_offer_and_candidates(&self, ctx: &Context<Self>) -> Html {
        let encoded = self.get_serialized_offer_and_candidates();
        html! {
            <div>
                { "Give this code to the person you want to talk to:" }
                <div style="overflow-wrap: break-word; max-width: 867px;" >
                    <div style="font-size:12; margin-top:8; margin-bottom:8;" id="copy-elem"> {encoded} </div>
                    <button
                        onclick={ctx.link().callback(move |_| Msg::CopyToClipboard)}
                        class="msger-send-btn"
                        style="border-radius: 3px; padding: 10px; font-size: 1em; border: none; margin-left: 0px; margin-top: 6px; float: left;"
                        >
                        {"Copy to clipboard"}
                    </button>
                </div>

            </div>

        }
    }

    fn get_debug_html(&self) -> Html {
        let state = self.web_rtc_manager.borrow().get_state();

        let html = match state {
            State::Default => html! { <div style="font-size:8;"> { "|Default State|"} </div> },
            State::Server(connection_state) => html! {
                <div style="font-size:8;">
                    { "|Server|"}
                    { " |ice_gathering: "} { format!("{:?}|", connection_state.ice_gathering_state) }
                    { " |ice_connection: "} { format!("{:?}|", connection_state.ice_connection_state) }
                    { " |data_channel: "} { format!("{:?}|", connection_state.data_channel_state) }
                </div>
            },
            State::Client(connection_state) => html! {
                <div style="font-size:8;">
                    { "|Client|"}
                    { " |ice_gathering: "} { format!("{:?}|", connection_state.ice_gathering_state) }
                    { " |ice_connection: "} { format!("{:?}|", connection_state.ice_connection_state) }
                    { " |data_channel: "} { format!("{:?}|", connection_state.data_channel_state) }
                </div>
            },
        };

        html
    }

    fn copy_content_to_clipboard(&self) {
        let window = web_sys::window().unwrap();
        let document = window.document().unwrap();
        let aux = document.create_element("input").unwrap();
        let aux = aux.dyn_into::<web_sys::HtmlInputElement>().unwrap();
        let content: String = document
            .get_element_by_id("copy-elem")
            .unwrap()
            .inner_html();
        let _result = aux.set_attribute("value", &content);
        let document = window.document().unwrap();
        let _result = document.body().unwrap().append_child(&aux);
        aux.select();
        let html_document = document.dyn_into::<web_sys::HtmlDocument>().unwrap();
        let _result = html_document.exec_command("copy");
        let document = window.document().unwrap();
        let _result = document.body().unwrap().remove_child(&aux);
    }

    fn get_messages_as_html(&self) -> Html {
        html! {
            <ul class="item-list">
                {
                    for self.messages.iter().map(|a_message|
                    {
                        let message_class = if a_message.sender == MessageSender::Other { "msg left-msg" } else { "msg right-msg" };
                        let message_sender_name = if a_message.sender == MessageSender::Other { "Friend" } else { "Me" };
                        html! {
                            <div class={message_class}>

                                <div class="msg-bubble">
                                    <div class="msg-info">
                                        <div class="msg-info-name">{ message_sender_name }</div>
                                    </div>

                                    <div class="msg-text">
                                        { a_message.content.clone() }
                                    </div>
                                </div>
                            </div>
                        }
                    })
                }
            </ul>
        }
    }
}

fn get_debug_state_string(state: &State) -> String {
    match state {
        State::Default => "Default State".into(),
        State::Server(connection_state) => format!(
            "{}\nice gathering: {:?}\nice connection: {:?}\ndata channel: {:?}\n",
            "Server",
            connection_state.ice_gathering_state,
            connection_state.ice_connection_state,
            connection_state.data_channel_state,
        ),

        State::Client(connection_state) => format!(
            "{}\nice gathering: {:?}\nice connection: {:?}\ndata channel: {:?}\n",
            "Client",
            connection_state.ice_gathering_state,
            connection_state.ice_connection_state,
            connection_state.data_channel_state,
        ),
    }
}
