use shared::protocol::{ClientMessage, PROTOCOL_VERSION, decode_server_msg, encode_client_msg};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{BinaryType, MessageEvent, WebSocket, console};

pub struct NetClient {
    _ws: WebSocket,
    _onopen: Closure<dyn FnMut()>,
    _onmessage: Closure<dyn FnMut(MessageEvent)>,
}

impl NetClient {
    pub fn connect(url: &str, session_token: String) -> Result<Self, String> {
        let ws = WebSocket::new(url).map_err(|e| format!("{e:?}"))?;
        ws.set_binary_type(BinaryType::Arraybuffer);

        let ws_open = ws.clone();
        let onopen = Closure::<dyn FnMut()>::new(move || {
            let msg = ClientMessage::Handshake {
                protocol_version: PROTOCOL_VERSION,
                session_token: session_token.clone(),
            };
            if let Ok(bytes) = encode_client_msg(&msg) {
                let _ = ws_open.send_with_u8_array(&bytes);
            }
        });
        ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));

        let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |ev: MessageEvent| {
            if let Ok(buf) = ev.data().dyn_into::<js_sys::ArrayBuffer>() {
                let arr = js_sys::Uint8Array::new(&buf);
                let bytes = arr.to_vec();
                if let Ok(msg) = decode_server_msg(&bytes) {
                    console::log_1(&format!("[ws] {msg:?}").into());
                }
            }
        });
        ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));

        Ok(Self {
            _ws: ws,
            _onopen: onopen,
            _onmessage: onmessage,
        })
    }
}
