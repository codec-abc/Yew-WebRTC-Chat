use wasm_bindgen::{JsCast, JsValue};

use crate::chat::chat_model::{ChatModel, ConnectionString, Msg};

use std::cell::RefCell;
use std::rc::Rc;
use std::str;

use base64;
use js_sys::{Array, Object, Reflect, JSON};
use serde::{Deserialize, Serialize};
use wasm_bindgen::closure::Closure;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    console, RtcConfiguration, RtcDataChannel, RtcDataChannelEvent, RtcDataChannelInit,
    RtcDataChannelState, RtcIceCandidate, RtcIceCandidateInit, RtcIceConnectionState,
    RtcIceGatheringState, RtcPeerConnection, RtcPeerConnectionIceEvent, RtcSessionDescriptionInit,
};

use crate::{Message, MessageSender};
use yew::html::Scope;

type SingleArgClosure = Closure<dyn FnMut(JsValue)>;
type SingleArgJsFn = Box<dyn FnMut(JsValue)>;

const STUN_SERVER: &str = "stun:stun.l.google.com:19302";

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct ConnectionState {
    pub ice_gathering_state: Option<RtcIceGatheringState>,
    pub ice_connection_state: Option<RtcIceConnectionState>,
    pub data_channel_state: Option<RtcDataChannelState>,
}

impl ConnectionState {
    pub fn new() -> ConnectionState {
        ConnectionState {
            ice_gathering_state: None,
            ice_connection_state: None,
            data_channel_state: None,
        }
    }
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum OfferError {
    InvalidBase64,
    InvalidString,
    SerializationError,
    InvalidOffer,
    //InvalidCandidate,
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum State {
    Default,
    Server(ConnectionState),
    Client(ConnectionState),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IceCandidate {
    candidate: String,
    sdp_mid: String,
    sdp_m_line_index: u16,
}

pub trait NetworkManager {
    fn new(link: &Scope<ChatModel<Self>>) -> Rc<RefCell<Self>>
    where
        Self: Sized;
    fn send_message(&self, message_content: &str);
    fn get_state(&self) -> State;
    fn set_state(&mut self, new_state: State);
    fn get_offer(&self) -> Option<String>;
    fn get_ice_candidates(&self) -> Vec<IceCandidate>;
    fn validate_offer(web_rtc_manager: Rc<RefCell<Self>>, str: &str) -> Result<(), OfferError>;
    fn validate_answer(web_rtc_manager: Rc<RefCell<Self>>, str: &str) -> Result<(), OfferError>;
    fn start_web_rtc(web_rtc_manager: Rc<RefCell<Self>>) -> Result<(), JsValue>;
}

pub struct WebRTCManager {
    state: State,
    rtc_peer_connection: Option<RtcPeerConnection>,
    data_channel: Option<RtcDataChannel>,
    exit_offer_or_answer_early: bool,
    ice_candidates: Vec<IceCandidate>,
    offer: Option<String>,
    parent_link: Scope<ChatModel<Self>>,
}

impl NetworkManager for WebRTCManager {
    fn new(link: &Scope<ChatModel<Self>>) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(WebRTCManager {
            state: State::Default,
            rtc_peer_connection: None,
            data_channel: None,
            ice_candidates: Vec::new(),
            offer: None,
            parent_link: link.clone(),
            exit_offer_or_answer_early: false,
        }))
    }

    fn send_message(&self, message_content: &str) {
        self.data_channel
            .as_ref()
            .expect("must have a data channel")
            .send_with_str(message_content)
            .expect("channel is open");

        //TODO error handling ?
    }

    fn get_state(&self) -> State {
        self.state.clone()
    }

    fn set_state(&mut self, new_state: State) {
        self.state = new_state;
    }

    fn get_offer(&self) -> Option<String> {
        self.offer.clone()
    }

    fn get_ice_candidates(&self) -> Vec<IceCandidate> {
        self.ice_candidates.clone()
    }

    fn validate_offer(
        web_rtc_manager: Rc<RefCell<WebRTCManager>>,
        str: &str,
    ) -> Result<(), OfferError> {
        let connection_string = WebRTCManager::parse_base64_str_to_connection(str);

        if connection_string.is_err() {
            return Err(connection_string.err().unwrap());
        }

        let connection_string = connection_string.ok().unwrap();

        let remote_description_js_value: JsValue =
            JSON::parse(&connection_string.offer).expect("Expected valid json");

        let remote_description =
            remote_description_js_value.unchecked_into::<RtcSessionDescriptionInit>();

        let web_rtc_manager_rc_clone = web_rtc_manager.clone();

        let create_answer_function: Box<dyn FnMut(JsValue)> = Box::new(move |a: JsValue| {
            let connection_string = Rc::new(connection_string.clone());
            let clone = web_rtc_manager_rc_clone.clone();

            let set_candidates_function: SingleArgJsFn = Box::new(move |_: JsValue| {
                WebRTCManager::set_candidates(clone.clone(), &*connection_string);
            });

            let set_candidates_closure = Closure::wrap(set_candidates_function);
            let web_rtc_manager_rc_clone_for_error_handler = web_rtc_manager_rc_clone.clone();

            let create_answer_exception_handler =
                Closure::wrap(Box::new(move |_send_channel: JsValue| {
                    web_rtc_manager_rc_clone_for_error_handler
                        .borrow_mut()
                        .exit_offer_or_answer_early = true;

                    console::log_1(&"Exception handler !".into());
                    console::log_1(&a);

                    web_sys::Window::alert_with_message(
                        &web_sys::window().unwrap(),
                        &"Promise create_answer encountered an exception. See console for details"
                            .to_string(),
                    )
                    .expect("alert should work");
                }) as SingleArgJsFn);

            let web_rtc_manager_rc_clone_clone = web_rtc_manager_rc_clone.clone();

            let set_local_description_closure = Closure::wrap(Box::new(move |answer: JsValue| {
                let answer = answer.unchecked_into::<RtcSessionDescriptionInit>();

                let set_local_description_exception_handler = WebRTCManager::get_exception_handler(
                    web_rtc_manager_rc_clone_clone.clone(),
                    "set_local_description closure has encountered an exception".into(),
                );

                console::log_1(&"setting local description".into());

                let _promise = web_rtc_manager_rc_clone_clone
                    .borrow()
                    .rtc_peer_connection
                    .as_ref()
                    .unwrap()
                    .set_local_description(&answer)
                    .catch(&set_local_description_exception_handler);

                console::log_1(&answer.clone().into());

                web_rtc_manager_rc_clone_clone.borrow_mut().offer =
                    Some(String::from(JSON::stringify(&answer).unwrap()));
            }) as SingleArgJsFn);

            // TODO: .await this
            _ = JsFuture::from(
                web_rtc_manager_rc_clone
                    .borrow()
                    .rtc_peer_connection
                    .as_ref()
                    .unwrap()
                    .create_answer()
                    .then(&set_local_description_closure)
                    .catch(&create_answer_exception_handler)
                    .then(&set_candidates_closure),
            );

            set_candidates_closure.forget();
            set_local_description_closure.forget();
        });

        let create_answer_closure = Closure::wrap(create_answer_function);

        let web_rtc_manager_rc_clone = web_rtc_manager.clone();
        let set_remote_description_exception_handler =
            Closure::wrap(Box::new(move |_send_channel: JsValue| {
                web_rtc_manager_rc_clone
                    .borrow_mut()
                    .exit_offer_or_answer_early = true;
            }) as SingleArgJsFn);

        let _promise = web_rtc_manager
            .borrow()
            .rtc_peer_connection
            .as_ref()
            .unwrap()
            .set_remote_description(&remote_description)
            .catch(&set_remote_description_exception_handler)
            .then(&create_answer_closure);

        create_answer_closure.forget();

        Ok(())
    }

    fn validate_answer(
        web_rtc_manager: Rc<RefCell<WebRTCManager>>,
        str: &str,
    ) -> Result<(), OfferError> {
        let connection_string = WebRTCManager::parse_base64_str_to_connection(str);

        if connection_string.is_err() {
            return Err(connection_string.err().unwrap());
        }

        let connection_string = connection_string.ok().unwrap();

        let remote_description_js_value: JsValue =
            JSON::parse(&connection_string.offer).expect("Expected valid json");

        let remote_description =
            remote_description_js_value.unchecked_into::<RtcSessionDescriptionInit>();

        let web_rtc_manager_rc_clone = web_rtc_manager.clone();

        let set_remote_description_exception_handler = Closure::wrap(Box::new(move |a: JsValue| {
            web_rtc_manager_rc_clone
                .borrow_mut()
                .exit_offer_or_answer_early = true;

            console::log_1(&"Exception handler !".into());
            console::log_1(&a);

            web_sys::Window::alert_with_message(
                &web_sys::window().unwrap(),
                &"Promise set_remote_description encountered an exception. See console for details"
                    .to_string(),
            )
            .expect("alert should work");

            web_rtc_manager_rc_clone
                .borrow()
                .parent_link
                .send_message(Msg::ResetWebRTC);
        }) as SingleArgJsFn);

        let connection_string = Rc::new(connection_string);
        let web_rtc_manager_rc_clone = web_rtc_manager.clone();
        let set_candidates_function: SingleArgJsFn = Box::new(move |_: JsValue| {
            WebRTCManager::set_candidates(web_rtc_manager_rc_clone.clone(), &*connection_string);
        });
        let set_candidates_closure = Closure::wrap(set_candidates_function);

        let _promise = web_rtc_manager
            .borrow()
            .rtc_peer_connection
            .as_ref()
            .unwrap()
            .set_remote_description(&remote_description)
            .catch(&set_remote_description_exception_handler)
            .then(&set_candidates_closure);

        set_candidates_closure.forget();

        Ok(())
    }

    fn start_web_rtc(web_rtc_manager: Rc<RefCell<WebRTCManager>>) -> Result<(), JsValue> {
        let rtc_peer_connection = {
            let ice_servers = Array::new();
            {
                let server_entry = Object::new();

                Reflect::set(&server_entry, &"urls".into(), &STUN_SERVER.into())?;

                ice_servers.push(&*server_entry);
            }

            let mut rtc_configuration = RtcConfiguration::new();
            rtc_configuration.ice_servers(&ice_servers);

            RtcPeerConnection::new_with_configuration(&rtc_configuration)?
        };

        let create_offer_exception_handler = WebRTCManager::get_exception_handler(
            web_rtc_manager.clone(),
            "create_offer closure has encountered an exception".into(),
        );

        let state = web_rtc_manager.borrow().state.clone();

        match state {
            State::Server(_connection_state) => {
                let web_rtc_manager_rc_clone = web_rtc_manager.clone();

                let mut data_channel_init = RtcDataChannelInit::new();
                data_channel_init.ordered(true);

                let data_channel: RtcDataChannel = rtc_peer_connection
                    .create_data_channel_with_data_channel_dict("sendChannel", &data_channel_init);

                WebRTCManager::set_data_channel(web_rtc_manager.clone(), data_channel);

                let create_offer_function: SingleArgJsFn = Box::new(move |offer: JsValue| {
                    let rtc_session_description: RtcSessionDescriptionInit =
                        offer.unchecked_into::<RtcSessionDescriptionInit>();

                    console::log_1(&rtc_session_description.clone().into());

                    web_rtc_manager_rc_clone.borrow_mut().offer = Some(String::from(
                        JSON::stringify(&rtc_session_description).unwrap(),
                    ));

                    let set_local_description_exception_handler =
                        WebRTCManager::get_exception_handler(
                            web_rtc_manager_rc_clone.clone(),
                            "set_local_description closure has encountered an exception".into(),
                        );

                    let _promise = web_rtc_manager_rc_clone
                        .borrow_mut()
                        .rtc_peer_connection
                        .as_ref()
                        .unwrap()
                        .set_local_description(&rtc_session_description)
                        .catch(&set_local_description_exception_handler);
                });

                let create_offer_closure = Closure::wrap(create_offer_function);

                let _create_offer_promise = rtc_peer_connection
                    .create_offer()
                    .then(&create_offer_closure)
                    .catch(&create_offer_exception_handler);

                create_offer_closure.forget();
            }

            State::Client(_connection_state) => {
                let clone = web_rtc_manager.clone();

                let on_data_channel_closure =
                    Closure::wrap(Box::new(move |data_channel_event: JsValue| {
                        let data_channel_event =
                            data_channel_event.unchecked_into::<RtcDataChannelEvent>();
                        let data_channel = data_channel_event.channel();
                        WebRTCManager::set_data_channel(clone.clone(), data_channel);
                    }) as SingleArgJsFn);

                rtc_peer_connection
                    .set_ondatachannel(Some(on_data_channel_closure.as_ref().unchecked_ref()));

                on_data_channel_closure.forget();
            }

            _ => {
                panic!("Not implemented");
            }
        };

        let web_rtc_manager_argument = web_rtc_manager.clone();
        let on_ice_candidate_closure =
            Closure::wrap(Box::new(move |ice_connection_event: JsValue| {
                console::log_1(&ice_connection_event);

                let ice_connection_event_obj: RtcPeerConnectionIceEvent =
                    ice_connection_event.unchecked_into::<RtcPeerConnectionIceEvent>();

                if let Some(candidate) = ice_connection_event_obj.candidate() {
                    let candidate_str = candidate.candidate();

                    if !candidate_str.is_empty() {
                        console::log_1(&candidate_str.clone().into());

                        let saved_candidate = IceCandidate {
                            candidate: candidate_str,
                            sdp_mid: candidate.sdp_mid().unwrap(),
                            sdp_m_line_index: candidate.sdp_m_line_index().unwrap(),
                        };

                        web_rtc_manager_argument
                            .borrow_mut()
                            .ice_candidates
                            .push(saved_candidate);
                    }
                }
            }) as SingleArgJsFn);

        let on_ice_connection_state_change_closure =
            WebRTCManager::get_on_ice_connection_state_change_closure(web_rtc_manager.clone());

        let on_ice_gathering_state_change_closure =
            WebRTCManager::get_on_ice_gathering_state_change_closure(web_rtc_manager.clone());

        rtc_peer_connection
            .set_onicecandidate(Some(on_ice_candidate_closure.as_ref().unchecked_ref()));

        rtc_peer_connection.set_oniceconnectionstatechange(Some(
            on_ice_connection_state_change_closure
                .as_ref()
                .unchecked_ref(),
        ));

        rtc_peer_connection.set_onicegatheringstatechange(Some(
            on_ice_gathering_state_change_closure
                .as_ref()
                .unchecked_ref(),
        ));

        web_rtc_manager.borrow_mut().rtc_peer_connection = Some(rtc_peer_connection);

        on_ice_candidate_closure.forget();
        on_ice_connection_state_change_closure.forget();
        on_ice_gathering_state_change_closure.forget();

        Ok(())
    }
}

impl WebRTCManager {
    // TODO : handle error when adding ice_candidate
    fn set_candidates(
        web_rtc_manager: Rc<RefCell<WebRTCManager>>,
        connection_string: &ConnectionString,
    ) {
        if web_rtc_manager.borrow().exit_offer_or_answer_early {
            return;
        }

        for candidate in &connection_string.ice_candidates {
            let mut ice_candidate_init = RtcIceCandidateInit::new("");

            ice_candidate_init.candidate(&candidate.candidate);
            ice_candidate_init.sdp_mid(Some(&candidate.sdp_mid));
            ice_candidate_init.sdp_m_line_index(Some(candidate.sdp_m_line_index));

            let ice_candidate = RtcIceCandidate::new(&ice_candidate_init).expect("valid candidate");

            let add_candidate_exception_handler = WebRTCManager::get_exception_handler(
                web_rtc_manager.clone(),
                "add_candidate closure has encountered an exception".into(),
            );

            let _promise = web_rtc_manager
                .borrow()
                .rtc_peer_connection
                .as_ref()
                .unwrap()
                .add_ice_candidate_with_opt_rtc_ice_candidate(Some(&ice_candidate))
                .catch(&add_candidate_exception_handler);
        }
    }

    fn parse_base64_str_to_connection(str: &str) -> Result<ConnectionString, OfferError> {
        base64::decode(str)
            .map_err(|_| OfferError::InvalidBase64)
            .and_then(|a| {
                let to_str = str::from_utf8(&a);
                match to_str {
                    Ok(a) => Ok(a.to_string()),
                    Err(_) => Err(OfferError::InvalidString),
                }
            })
            .and_then(|a_str| {
                serde_json::from_str::<ConnectionString>(&a_str)
                    .map_err(|_| OfferError::SerializationError)
            })
            .and_then(|connection_string| {
                let remote_description = JSON::parse(&connection_string.offer);
                if remote_description.is_err() {
                    // TODO : additional check
                    return Err(OfferError::InvalidOffer);
                }

                // TODO : additional check
                Ok(connection_string)
            })
    }

    fn get_channel_status_change_closure(
        web_rtc_manager: Rc<RefCell<WebRTCManager>>,
    ) -> SingleArgClosure {
        Closure::wrap(Box::new(move |_send_channel: JsValue| {
            let state = web_rtc_manager
                .borrow()
                .data_channel
                .as_ref()
                .unwrap()
                .ready_state();

            let self_state = web_rtc_manager.borrow().get_state();

            let new_state = match self_state {
                State::Server(mut connection_state) => {
                    connection_state.data_channel_state = Some(state);
                    State::Server(connection_state)
                }
                State::Client(mut connection_state) => {
                    connection_state.data_channel_state = Some(state);
                    State::Client(connection_state)
                }
                a => a,
            };

            web_rtc_manager.borrow_mut().set_state(new_state);

            let web_rtc_state = web_rtc_manager.borrow().get_state();

            web_rtc_manager
                .borrow()
                .parent_link
                .send_message(Msg::UpdateWebRTCState(web_rtc_state));
        }) as SingleArgJsFn)
    }

    fn get_on_data_closure(web_rtc_manager: Rc<RefCell<WebRTCManager>>) -> SingleArgClosure {
        Closure::wrap(Box::new(move |arg: JsValue| {
            let message_event = arg.unchecked_into::<web_sys::MessageEvent>();

            let msg_content: String = message_event.data().as_string().unwrap();
            let msg = Message::new(msg_content, MessageSender::Other);

            web_rtc_manager
                .borrow()
                .parent_link
                .send_message(Msg::NewMessage(msg));
        }) as SingleArgJsFn)
    }

    fn get_on_ice_connection_state_change_closure(
        web_rtc_manager: Rc<RefCell<WebRTCManager>>,
    ) -> SingleArgClosure {
        Closure::wrap(Box::new(move |_ice_connection_state_event: JsValue| {
            let ice_new_state: RtcIceConnectionState = {
                let inner = web_rtc_manager.borrow();
                let connection: &RtcPeerConnection = inner.rtc_peer_connection.as_ref().unwrap();
                connection.ice_connection_state()
            };

            let self_state = web_rtc_manager.borrow().get_state();

            let new_state = match self_state {
                State::Server(mut connection_state) => {
                    connection_state.ice_connection_state = Some(ice_new_state);
                    State::Server(connection_state)
                }
                State::Client(mut connection_state) => {
                    connection_state.ice_connection_state = Some(ice_new_state);
                    State::Client(connection_state)
                }
                a => a,
            };

            web_rtc_manager.borrow_mut().set_state(new_state);

            let web_rtc_state = web_rtc_manager.borrow().get_state();

            web_rtc_manager
                .borrow()
                .parent_link
                .send_message(Msg::UpdateWebRTCState(web_rtc_state));
        }) as SingleArgJsFn)
    }

    fn get_on_ice_gathering_state_change_closure(
        web_rtc_manager: Rc<RefCell<WebRTCManager>>,
    ) -> SingleArgClosure {
        Closure::wrap(Box::new(move |_ice_gathering_state: JsValue| {
            let ice_new_state: RtcIceGatheringState = {
                let inner = web_rtc_manager.borrow();
                let connection: &RtcPeerConnection = inner.rtc_peer_connection.as_ref().unwrap();
                connection.ice_gathering_state()
            };

            let self_state = web_rtc_manager.borrow().get_state();

            let new_state = match self_state {
                State::Server(mut connection_state) => {
                    connection_state.ice_gathering_state = Some(ice_new_state);
                    State::Server(connection_state)
                }
                State::Client(mut connection_state) => {
                    connection_state.ice_gathering_state = Some(ice_new_state);
                    State::Client(connection_state)
                }
                a => a,
            };

            web_rtc_manager.borrow_mut().set_state(new_state);
            let web_rtc_state = web_rtc_manager.borrow().get_state();

            web_rtc_manager
                .borrow()
                .parent_link
                .send_message(Msg::UpdateWebRTCState(web_rtc_state));
        }) as SingleArgJsFn)
    }

    fn get_exception_handler(
        _web_rtc_manager: Rc<RefCell<WebRTCManager>>,
        message: String,
    ) -> SingleArgClosure {
        Closure::wrap(Box::new(move |a: JsValue| {
            // TODO
            console::log_1(&"Exception handler !".into());
            console::log_1(&JsValue::from_str(&message));
            console::log_1(&a);

            web_sys::Window::alert_with_message(
                &web_sys::window().unwrap(),
                &"Promise encountered an exception. See console for details".to_string(),
            )
            .expect("alert should work");
        }) as SingleArgJsFn)
    }

    fn set_data_channel(web_rtc_manager: Rc<RefCell<WebRTCManager>>, data_channel: RtcDataChannel) {
        let channel_status_change_closure =
            WebRTCManager::get_channel_status_change_closure(web_rtc_manager.clone());

        data_channel.set_onopen(Some(channel_status_change_closure.as_ref().unchecked_ref()));
        data_channel.set_onclose(Some(channel_status_change_closure.as_ref().unchecked_ref()));

        channel_status_change_closure.forget();

        let on_data_closure = WebRTCManager::get_on_data_closure(web_rtc_manager.clone());
        data_channel.set_onmessage(Some(on_data_closure.as_ref().unchecked_ref()));

        on_data_closure.forget();

        web_rtc_manager.borrow_mut().data_channel = Some(data_channel);
    }
}
