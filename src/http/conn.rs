use std::io::Cursor;
use std::sync::mpsc;

use tick::{Transfer, Protocol};

use http::h1::{self as http, Incoming, TryParse};

const MAX_BUFFER_SIZE: usize = 8192 + 4096 * 100;

pub struct Conn<H: Handler> {
    transfer: Lend<Transfer>,
    state: State,
    buffer: Cursor<Vec<u8>>,
    handler: H,
}


impl<H: Handler> Conn<H> {
    pub fn new(transfer: Transfer, handler: H) -> Conn<H> {
        Conn {
            transfer: Lend::Owned(transfer),
            state: State::Parsing,
            buffer: Cursor::new(Vec::with_capacity(4096)),
            handler: handler,
        }
    }
}

impl<H: Handler> Protocol for Conn<H> {
    fn on_data(&mut self, data: &[u8]) {
        if self.transfer.claim().is_some() {
            self.state = State::Parsing;
        }
        match self.state {
            State::Parsing => {
                info!("on_data parse {}", data.len());
                self.buffer.get_mut().extend(data);
                match http::parse::<H::Parse, _>(self.buffer.get_ref()) {
                    Ok(Some((incoming, len))) => {
                        self.buffer.set_position(len as u64);
                        let lease = self.transfer.lease();
                        self.handler.on_incoming(incoming, lease);
                        self.state = State::Handling;
                    },
                    Ok(None) => {
                        trace!("TODO: check MAX_LENGTH {}", self.buffer.get_ref().len());
                    },
                    Err(e) => {
                        //TODO: match on error to send proper response
                        //TODO: have Handler.on_parse_error() or something
                        self.transfer.claim().expect("lost transfer").close();
                        self.state = State::Closed;
                    }
                };
            }
            State::Handling => {
                info!("on_data handle {}", data.len());
                let used = self.handler.on_body(data);
                if data.len() > used {
                    self.buffer.get_mut().truncate(0);
                    self.buffer.set_position(0);
                    self.buffer.get_mut().extend(&data[used..]);
                    self.state = State::Parsing;
                }
            },
            State::Closed => unimplemented!()
        }
    }
}

pub trait Handler {
    type Parse: TryParse;
    fn on_incoming(&mut self,
                   incoming: Incoming<<Self::Parse as TryParse>::Subject>,
                   transfer: Lease<Transfer>);

    fn on_body(&mut self, data: &[u8]) -> usize;
}

enum State {
    Parsing,
    Handling,
    Closed,
}

enum Lend<T> {
    Owned(T),
    Lent(mpsc::Receiver<T>)
}

impl<T> Lend<T> {
    fn lease(&mut self) -> Lease<T> {
        let (tx, rx) = mpsc::channel();
        match ::std::mem::replace(self, Lend::Lent(rx)) {
            Lend::Owned(t) => Lease::new(t, tx),
            _ => panic!("already leased")
        }
    }

    fn claim(&mut self) -> Option<&mut T> {
        match *self {
            Lend::Lent(ref rx) => {
                rx.try_recv().ok()
            }
            _ => None
        }.map(|t| *self = Lend::Owned(t));

        match *self {
            Lend::Owned(ref mut t) => Some(t),
            Lend::Lent(_) => None
        }

    }
}

pub struct Lease<T> {
    inner: Option<T>,
    tx: mpsc::Sender<T>,
}

impl<T> Lease<T> {
    pub fn new(inner: T, tx: mpsc::Sender<T>) -> Lease<T> {
        Lease {
            inner: Some(inner),
            tx: tx,
        }
    }
}

impl<T> ::std::ops::Deref for Lease<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.inner.as_ref().unwrap()
    }
}

impl<T> ::std::ops::DerefMut for Lease<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.inner.as_mut().unwrap()
    }
}

impl<T> Drop for Lease<T> {
    fn drop(&mut self) {
        self.inner.take().map(|t| self.tx.send(t));
    }
}
