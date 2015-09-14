use std::io::Cursor;
use std::sync::Arc;

use eventual::{self, Async, AsyncError, Complete, Future};
use tick::{Sender, Stream};

use header;
use http;

use super::{Handler, Request, Response};

const MAX_BUFFER_SIZE: usize = 8192 + 4096 * 100;

pub fn handle<H>(tick_tx: Sender<Vec<u8>>, tick_rx: Stream<Vec<u8>>, handler: Arc<H>)
where H: Handler + 'static {
    trace!("handling connection");
    parse(tick_rx).receive(move |res| {
        let (tick_tx, future_tx) = borrowed_sender(tick_tx);
        match res {
            Ok((req, future_rx)) => {
                let mut resp = Response::new(tick_tx);
                if http::should_keep_alive(req.version, &req.headers) {
                    resp.headers_mut().set(header::Connection::keep_alive());
                } else {
                    resp.headers_mut().set(header::Connection::close());
                }

                handler.handle(req, resp);
                eventual::join((future_tx, future_rx)).receive(move |res| {
                    match res {
                        Ok((tick_tx, tick_rx)) => {
                            handle(tick_tx, tick_rx, handler);
                        }
                        Err(_) => ()
                    }
                })
            },
            Err(AsyncError::Failed(e)) => {
                match e {
                    ::Error::Incomplete => {
                        trace!("parsing incomplete, dropping");
                    },
                    ::Error::Method |
                    ::Error::Uri(_) |
                    ::Error::Version |
                    ::Error::Header => {
                        let mut resp = Response::new(tick_tx);
                        *resp.status_mut() = ::status::StatusCode::BadRequest;
                    },
                    ::Error::TooLarge => {
                        let mut resp = Response::new(tick_tx);
                        *resp.status_mut() = ::status::StatusCode::PayloadTooLarge;
                    }
                    _ => {
                        error!("parse error = {:?}", e);
                    }
                }
            }
            Err(AsyncError::Aborted) => {
                trace!("parsing aborted");
            }
        }
    });
}

fn borrowed_sender(tx: Sender<Vec<u8>>) -> (Sender<Vec<u8>>, ::tick::Future<Sender<Vec<u8>>>) {
    let (sender, stream) = Stream::pair();
    (sender, tx.send_all(stream).map_err(|(e, _tx)| e))
}

fn borrowed_stream(rx: Stream<Vec<u8>>) -> (Stream<Vec<u8>>, ::tick::Future<Stream<Vec<u8>>>) {
    let (sender, stream) = Stream::pair();
    let (complete, future) = Future::pair();
    do_borrowed_stream(rx, sender, complete);
    (stream, future)
}

fn do_borrowed_stream<A>(rx: Stream<Vec<u8>>, tx: A, complete: Complete<Stream<Vec<u8>>, ::tick::Error>)
where A: Async<Value=Sender<Vec<u8>>> {
    // does Request want data yet?
    tx.receive(move |res| {
        match res {
            Ok(tx) => {
                // yes it does, ask tick for data
                rx.receive(move |res| {
                    match res {
                        Ok(Some((vec, stream))) => {
                            do_borrowed_stream(stream, tx.send(vec), complete);
                        },
                        Ok(None) => {
                            trace!("tick dropped this tcpstream");
                        },
                        Err(e) => {
                            if let Some(e) = e.take() {
                                complete.fail(e)
                            }
                        }
                    }
                });
            }
            Err(_) => {
                // nope, it's dropped. return to Connection
                trace!("borrowed stream dropped, returning");
                complete.complete(rx);
            }
        }
    });
}

fn parse(stream: Stream<Vec<u8>>) -> Future<(Request, ::tick::Future<Stream<Vec<u8>>>), ::Error> {
    let (defer, future) = Future::pair();
    defer.receive(move |result| {
        if let Ok(defer) = result {
            do_parse(vec![], stream, defer);
        }
    });
    future
}

fn do_parse(mut buf: Vec<u8>, stream: Stream<Vec<u8>>, defer: Complete<(Request, ::tick::Future<Stream<Vec<u8>>>), ::Error>) {
    use http::h1;
    stream.receive(move |result| {
        match result {
            Ok(Some((bytes, stream))) => {
                buf.extend(&bytes);
                match h1::parse_request(&buf) {
                    Ok(Some((incoming, pos))) => {
                        let mut buf = Cursor::new(buf);
                        buf.set_position(pos as u64);
                        let (stream, future_rx) = borrowed_stream(stream);
                        let request = Request::new(incoming, buf, stream);
                        defer.complete((request, future_rx));
                    }
                    Ok(None) => {
                        if buf.len() < MAX_BUFFER_SIZE {
                            do_parse(buf, stream, defer);
                        } else {
                            defer.fail(::Error::TooLarge);
                        }
                    },
                    Err(e) => defer.fail(e)
                }

            },
            Ok(None) => {
                // eof before parsing succeeded... error
                trace!("eof parsing");
                defer.fail(::Error::Incomplete)
            }
            Err(::eventual::AsyncError::Failed(e)) => defer.fail(From::from(e)),
            Err(::eventual::AsyncError::Aborted) => {
                trace!("tick stream aborted!");
            }
        }
    });
}


