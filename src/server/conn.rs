use std::io::Cursor;
use std::sync::Arc;

use httparse;

use header;
use http::{self, Transfer};
use method::Method;
use uri::RequestUri;

use super::{Handler, Request, Response};

pub struct Conn<H: Handler> {
    handler: Arc<H>
}

impl<H: Handler> Conn<H> {
    pub fn new(handler: Arc<H>) -> Conn<H> {
        Conn {
            handler: handler,
        }
    }
}

impl<H: Handler> http::Handler for Conn<H> {
    type Parse = httparse::Request<'static, 'static>;

    fn on_incoming(&mut self, incoming: http::h1::Incoming<(Method, RequestUri)>, transfer: Transfer) {
        let request = Request::new(incoming);
        let response = Response::new(transfer);
        self.handler.handle(request, response);
    }

    fn on_body(&mut self, data: &[u8]) -> usize {
        0
    }
}
