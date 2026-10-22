//! The browser build's [`Datagrams`]: a WebSocket to the master server's relay, which passes
//! each binary message on as one UDP datagram to the game server and each datagram back as
//! one message (docs/web.md). The relay knows the server's address; here the one peer is
//! [`PEER`].

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddr};
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{BinaryType, CloseEvent, MessageEvent, WebSocket};

use crate::transport::Datagrams;

/// The relay's address, as far as the connection layer is concerned.
pub const PEER: SocketAddr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);
/// Messages kept until the next poll, at most (the rest are lost, like datagrams).
const MAX_WAITING: usize = 4096;

#[derive(Default)]
struct Inbox {
    packets: VecDeque<Vec<u8>>,
    opened: bool,
    closed: Option<String>,
}

pub struct WsDatagrams {
    ws: WebSocket,
    inbox: Rc<RefCell<Inbox>>,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_open: Closure<dyn FnMut()>,
    _on_close: Closure<dyn FnMut(CloseEvent)>,
}

impl WsDatagrams {
    /// Opens the WebSocket to `url`; it connects in the background.
    pub fn open(url: &str) -> std::io::Result<Self> {
        let ws = WebSocket::new(url).map_err(|e| std::io::Error::other(format!("{url}: {e:?}")))?;
        ws.set_binary_type(BinaryType::Arraybuffer);
        let inbox = Rc::new(RefCell::new(Inbox::default()));
        let to = inbox.clone();
        let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
            if let Ok(buf) = e.data().dyn_into::<js_sys::ArrayBuffer>() {
                let mut inbox = to.borrow_mut();
                if inbox.packets.len() < MAX_WAITING {
                    inbox.packets.push_back(js_sys::Uint8Array::new(&buf).to_vec());
                }
            }
        });
        let to = inbox.clone();
        let on_open = Closure::<dyn FnMut()>::new(move || to.borrow_mut().opened = true);
        let to = inbox.clone();
        let on_close = Closure::<dyn FnMut(CloseEvent)>::new(move |e: CloseEvent| {
            let mut inbox = to.borrow_mut();
            let reason = e.reason();
            // A browser doesn't tell why a handshake was turned down.
            inbox.closed = Some(if !reason.is_empty() {
                format!("Relay: {reason}")
            } else if inbox.opened {
                "The relay closed the connection".to_string()
            } else {
                "The relay turned the connection down: the server is off the list, or there \
                 are too many connections from here"
                    .to_string()
            });
        });
        ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        Ok(Self { ws, inbox, _on_message: on_message, _on_open: on_open, _on_close: on_close })
    }
}

// SAFETY: the browser build has one thread (no `atomics`), so nothing is ever sent or shared
// across threads; Bevy only asks for Send and Sync to keep it in a resource.
#[cfg(not(target_feature = "atomics"))]
unsafe impl Send for WsDatagrams {}
#[cfg(not(target_feature = "atomics"))]
unsafe impl Sync for WsDatagrams {}

impl Datagrams for WsDatagrams {
    fn recv_from(&mut self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        let p = self.inbox.borrow_mut().packets.pop_front();
        match p {
            Some(p) => {
                let n = p.len().min(buf.len());
                buf[..n].copy_from_slice(&p[..n]);
                Ok((n, PEER))
            }
            None => Err(ErrorKind::WouldBlock.into()),
        }
    }

    /// Lost while the WebSocket still connects (the connection layer asks again).
    fn send_to(&mut self, p: &[u8], _to: SocketAddr) {
        if self.ws.ready_state() == WebSocket::OPEN {
            let _ = self.ws.send_with_u8_array(p);
        }
    }

    fn closed(&self) -> Option<String> {
        self.inbox.borrow().closed.clone()
    }
}

impl Drop for WsDatagrams {
    fn drop(&mut self) {
        self.ws.set_onmessage(None);
        self.ws.set_onopen(None);
        self.ws.set_onclose(None);
        let _ = self.ws.close();
    }
}
