use std::io::Cursor;
use std::sync::Arc;

use eventual::{Async, Complete, Future};
use tick::{Sender, Stream};

use super::{Handler, Request, Response};

pub struct Connection {
    rx: Stream<Vec<u8>>,
    tx: Sender<Vec<u8>>,
}

impl Connection {
    pub fn new(rx: Stream<Vec<u8>>, tx: Sender<Vec<u8>>) -> Connection {
        Connection {
            rx: rx,
            tx: tx,
        }
    }
    
    pub fn handle<H: Handler + 'static>(self, handler: Arc<H>) {
        let mut resp = Response::new(self.tx);
        parse(self.rx).receive(move |res| {
            match res {
                Ok(request) => {
                    handler.handle(request, resp);
                }
                Err(e) => {
                    trace!("error parsing: {:?}", e);
                    *resp.status_mut() = ::status::StatusCode::BadRequest;
                }
            }
        })
    }
}

fn parse(stream: Stream<Vec<u8>>) -> Future<(Request, Future<Stream<Vec<u8>>, ::Error>), ::Error> {
    let (defer, future) = Future::pair();
    defer.receive(move |result| {
        if let Ok(defer) = result {
            do_parse(vec![], stream, defer);
        }
    });
    future
}

fn do_parse(mut buf: Vec<u8>, stream: Stream<Vec<u8>>, defer: Complete<(Request, Future<Stream<Vec<u8>>, ::Error>), ::Error>) {
    use http::h1;
    stream.receive(move |result| {
        match result {
            Ok(Some((bytes, stream))) => {
                buf.extend(&bytes);
                match h1::parse_request(&buf) {
                    Ok(Some((incoming, pos))) => {
                        let mut buf = Cursor::new(buf);
                        buf.set_position(pos as u64);
                        let request = Request::new(incoming, buf, stream);
                        defer.complete(request);
                    }
                    Ok(None) => {
                        // if buf.len() < MAX_HEAD_SIZE
                        do_parse(buf, stream, defer);
                    },
                    Err(e) => defer.fail(e)
                }

            },
            Ok(None) => {
                // eof before parsing succeeded... error
            }
            Err(::eventual::AsyncError::Failed(e)) => defer.fail(From::from(e)),
            Err(::eventual::AsyncError::Aborted) => {
                trace!("what does this mean, aborted?");
            }
        }
    });
}


