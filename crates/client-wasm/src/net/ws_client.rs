use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use shared::protocol::{
    ClientMessage, PROTOCOL_VERSION, ServerMessage, decode_server_msg, encode_client_msg,
};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{BinaryType, MessageEvent, WebSocket, console};

pub struct NetClient {
    ws: WebSocket,
    session_token: String,
    inbound: Rc<RefCell<VecDeque<ServerMessage>>>,
    outbound_pending: Rc<RefCell<Vec<Vec<u8>>>>,
    closed: Rc<Cell<bool>>,
    _onopen: Closure<dyn FnMut()>,
    _onmessage: Closure<dyn FnMut(MessageEvent)>,
    _onclose: Closure<dyn FnMut()>,
    _onerror: Closure<dyn FnMut()>,
}

impl Drop for NetClient {
    fn drop(&mut self) {
        self.ws.set_onopen(None);
        self.ws.set_onmessage(None);
        self.ws.set_onclose(None);
        self.ws.set_onerror(None);
        let _ = self.ws.close();
    }
}

impl NetClient {
    pub fn connect(url: &str, session_token: String) -> Result<Self, String> {
        let ws = WebSocket::new(url).map_err(|e| format!("{e:?}"))?;
        ws.set_binary_type(BinaryType::Arraybuffer);

        let inbound = Rc::new(RefCell::new(VecDeque::new()));
        let outbound_pending: Rc<RefCell<Vec<Vec<u8>>>> = Rc::new(RefCell::new(Vec::new()));
        let closed = Rc::new(Cell::new(false));

        let ws_open = ws.clone();
        let token_open = session_token.clone();
        let pending_open = Rc::clone(&outbound_pending);
        let onopen = Closure::<dyn FnMut()>::new(move || {
            let msg = ClientMessage::Handshake {
                protocol_version: PROTOCOL_VERSION,
                session_token: token_open.clone(),
            };
            if let Ok(bytes) = encode_client_msg(&msg) {
                let _ = ws_open.send_with_u8_array(&bytes);
            }
            for queued in pending_open.borrow_mut().drain(..) {
                let _ = ws_open.send_with_u8_array(&queued);
            }
        });
        ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));

        let inbound_msg = Rc::clone(&inbound);
        let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |ev: MessageEvent| {
            if let Ok(buf) = ev.data().dyn_into::<js_sys::ArrayBuffer>() {
                let arr = js_sys::Uint8Array::new(&buf);
                let bytes = arr.to_vec();
                if let Ok(msg) = decode_server_msg(&bytes) {
                    if !matches!(msg, ServerMessage::WorldSnapshot { .. }) {
                        console::log_1(&format!("[ws] {msg:?}").into());
                    }
                    inbound_msg.borrow_mut().push_back(msg);
                }
            }
        });
        ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));

        let closed_close = Rc::clone(&closed);
        let onclose = Closure::<dyn FnMut()>::new(move || {
            closed_close.set(true);
        });
        ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));

        let closed_err = Rc::clone(&closed);
        let onerror = Closure::<dyn FnMut()>::new(move || {
            closed_err.set(true);
        });
        ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));

        Ok(Self {
            ws,
            session_token,
            inbound,
            outbound_pending,
            closed,
            _onopen: onopen,
            _onmessage: onmessage,
            _onclose: onclose,
            _onerror: onerror,
        })
    }

    pub fn session_token(&self) -> &str {
        &self.session_token
    }

    pub fn is_closed(&self) -> bool {
        self.closed.get() || self.ws.ready_state() == WebSocket::CLOSED
    }

    pub fn send(&self, msg: &ClientMessage) {
        let Ok(bytes) = encode_client_msg(msg) else {
            return;
        };
        match self.ws.ready_state() {
            WebSocket::OPEN => {
                let _ = self.ws.send_with_u8_array(&bytes);
            }
            WebSocket::CONNECTING => {
                self.outbound_pending.borrow_mut().push(bytes);
            }
            _ => {}
        }
    }

    pub fn drain_messages(&self, out: &mut Vec<ServerMessage>) {
        let mut q = self.inbound.borrow_mut();
        while let Some(msg) = q.pop_front() {
            out.push(msg);
        }
    }
}
