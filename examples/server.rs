#![deny(warnings)]
extern crate hyper;
extern crate env_logger;

extern crate eventual;
use eventual::Async;

use hyper::{Get, Post};
use hyper::header::ContentLength;
use hyper::server::{Server, Request, Response};
use hyper::uri::RequestUri::AbsolutePath;


fn echo(req: Request, mut res: Response) {
    match req.uri {
        AbsolutePath(ref path) => match (&req.method, &path[..]) {
            (&Get, "/") | (&Get, "/echo") => {
                res.send(b"Try POST /echo");
                return;
            },
            (&Post, "/echo") => (), // fall through, fighting mutable borrows
            _ => {
                *res.status_mut() = hyper::NotFound;
                return;
            }
        },
        _ => {
            return;
        }
    };

    if let Some(len) = req.headers.get::<ContentLength>() {
        res.headers_mut().set(*len);
    }

    let mut res = res.start();
    req.stream().each(move |data| {
        println!("data: {:?}", data);
        res.write(&data);
    }).fire();
    println!("handler end");
}

fn main() {
    env_logger::init().unwrap();
    let server = Server::http("127.0.0.1:1337").unwrap();
    let _guard = server.handle(echo);
    println!("Listening on http://127.0.0.1:1337");
}
