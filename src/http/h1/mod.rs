use std::marker::PhantomData;
use std::io::{Write, BufWriter};

use tick;
use time::now_utc;

use header::{self, Headers};
use http::{self, AsyncWriter};
use method::Method;
use net::{Fresh, Streaming};
use status::StatusCode;
use version::HttpVersion;

use self::write::HttpWriter;

pub use self::parse::parse;

mod parse;
mod read;
mod write;

fn should_have_response_body(method: &Method, status: u16) -> bool {
    trace!("should_have_response_body({:?}, {})", method, status);
    match (method, status) {
        (&Method::Head, _) |
        (_, 100...199) |
        (_, 204) |
        (_, 304) |
        (&Method::Connect, 200...299) => false,
        _ => true
    }
}

#[derive(Debug)]
pub struct Transfer<T, S> {
    body: HttpWriter<BufWriter<AsyncWriter>>,
    _type: PhantomData<T>,
    _state: PhantomData<S>,
}

pub fn transfer<T, S>(trans: http::conn::Lease<tick::Transfer>) -> Transfer<T, S> {
    Transfer {
        body: HttpWriter::ThroughWriter(BufWriter::with_capacity(4096, AsyncWriter::new(trans))),
        _type: PhantomData,
        _state: PhantomData,
    }
}

impl Transfer<http::Response, Fresh> {
    pub fn start(mut self, version: HttpVersion, status: StatusCode, headers: &mut Headers) -> Transfer<http::Response, Streaming> {
        debug!("writing head: {:?} {:?}", version, status);
        let _ = write!(&mut self.body, "{} {}\r\n", version, status);

        if !headers.has::<header::Date>() {
            headers.set(header::Date(header::HttpDate(now_utc())));
        }



        let mut body = Body::Chunked;
        if let Some(cl) = headers.get::<header::ContentLength>() {
            body = Body::Sized(**cl);
        }

        if body == Body::Chunked {
            let encodings = match headers.get_mut::<header::TransferEncoding>() {
                Some(&mut header::TransferEncoding(ref mut encodings)) => {
                    //TODO: check if chunked is already in encodings. use HashSet?
                    encodings.push(header::Encoding::Chunked);
                    false
                },
                None => true
            };

            if encodings {
                headers.set(header::TransferEncoding(vec![header::Encoding::Chunked]));
            }
            body = Body::Chunked;
        }


        debug!("{:#?}", headers);
        let _ = write!(&mut self.body, "{}\r\n", headers);

        let body = match body {
            Body::Sized(len) => HttpWriter::SizedWriter(self.body.into_inner(), len),
            Body::Chunked => HttpWriter::ChunkedWriter(self.body.into_inner())
        };

        Transfer {
            body: body,
            _type: PhantomData,
            _state: PhantomData
        }
    }
}

impl<T> Transfer<T, Streaming> {
    #[inline]
    pub fn write(&mut self, data: &[u8]) {
        let _ = self.body.write(data);
    }
}

#[derive(PartialEq, Debug)]
enum Body {
    Sized(u64),
    Chunked
}

/*
const MAX_INVALID_RESPONSE_BYTES: usize = 1024 * 128;
impl HttpMessage for Http11Message {
    fn set_outgoing(&mut self, mut head: RequestHead) -> ::Result<RequestHead> {
        let stream = match self.stream.take() {
            Some(stream) => stream,
            None => {
                return Err(From::from(io::Error::new(
                            io::ErrorKind::Other,
                            "Message not idle, cannot start new outgoing")));
            }
        };
        let mut stream = BufWriter::new(stream);

        let mut uri = head.url.serialize_path().unwrap();
        if let Some(ref q) = head.url.query {
            uri.push('?');
            uri.push_str(&q[..]);
        }

        let version = version::HttpVersion::Http11;
        debug!("request line: {:?} {:?} {:?}", head.method, uri, version);
        try!(write!(&mut stream, "{} {} {}{}",
                    head.method, uri, version, LINE_ENDING));

        let stream = {
            let mut write_headers = |mut stream: BufWriter<Box<NetworkStream + Send>>, head: &RequestHead| {
                debug!("headers={:?}", head.headers);
                match write!(&mut stream, "{}{}", head.headers, LINE_ENDING) {
                    Ok(_) => Ok(stream),
                    Err(e) => {
                        self.stream = Some(stream.into_inner().unwrap());
                        Err(e)
                    }
                }
            };
            match &head.method {
                &Method::Get | &Method::Head => {
                    EmptyWriter(try!(write_headers(stream, &head)))
                },
                _ => {
                    let mut chunked = true;
                    let mut len = 0;

                    match head.headers.get::<header::ContentLength>() {
                        Some(cl) => {
                            chunked = false;
                            len = **cl;
                        },
                        None => ()
                    };

                    // can't do in match above, thanks borrowck
                    if chunked {
                        let encodings = match head.headers.get_mut::<header::TransferEncoding>() {
                            Some(encodings) => {
                                //TODO: check if chunked is already in encodings. use HashSet?
                                encodings.push(header::Encoding::Chunked);
                                false
                            },
                            None => true
                        };

                        if encodings {
                            head.headers.set(
                                header::TransferEncoding(vec![header::Encoding::Chunked]))
                        }
                    }

                    let stream = try!(write_headers(stream, &head));

                    if chunked {
                        ChunkedWriter(stream)
                    } else {
                        SizedWriter(stream, len)
                    }
                }
            }
        };

        self.writer = Some(stream);
        self.method = Some(head.method.clone());

        Ok(head)
    }

    fn get_incoming(&mut self) -> ::Result<ResponseHead> {
        unimplemented!();
        /*
        try!(self.flush_outgoing());
        let stream = match self.stream.take() {
            Some(stream) => stream,
            None => {
                // The message was already in the reading state...
                // TODO Decide what happens in case we try to get a new incoming at that point
                return Err(From::from(
                        io::Error::new(io::ErrorKind::Other,
                        "Read already in progress")));
            }
        };

        let expected_no_content = stream.previous_response_expected_no_content();
        trace!("previous_response_expected_no_content = {}", expected_no_content);

        let mut stream = BufReader::new(stream);

        let mut invalid_bytes_read = 0;
        let head;
        loop {
            head = match parse_response(&mut stream) {
                Ok(head) => head,
                Err(::Error::Version)
                if expected_no_content && invalid_bytes_read < MAX_INVALID_RESPONSE_BYTES => {
                    trace!("expected_no_content, found content");
                    invalid_bytes_read += 1;
                    stream.consume(1);
                    continue;
                }
                Err(e) => {
                    self.stream = Some(stream.into_inner());
                    return Err(e);
                }
            };
            break;
        }

        let raw_status = head.subject;
        let headers = head.headers;

        let method = self.method.take().unwrap_or(Method::Get);

        let is_empty = !should_have_response_body(&method, raw_status.0);
        stream.get_mut().set_previous_response_expected_no_content(is_empty);
        // According to https://tools.ietf.org/html/rfc7230#section-3.3.3
        // 1. HEAD reponses, and Status 1xx, 204, and 304 cannot have a body.
        // 2. Status 2xx to a CONNECT cannot have a body.
        // 3. Transfer-Encoding: chunked has a chunked body.
        // 4. If multiple differing Content-Length headers or invalid, close connection.
        // 5. Content-Length header has a sized body.
        // 6. Not Client.
        // 7. Read till EOF.
        self.reader = Some(if is_empty {
            EmptyReader(stream)
        } else {
             if let Some(&TransferEncoding(ref codings)) = headers.get() {
                if codings.last() == Some(&Chunked) {
                    ChunkedReader(stream, None)
                } else {
                    trace!("not chuncked. read till eof");
                    EofReader(stream)
                }
            } else if let Some(&ContentLength(len)) =  headers.get() {
                SizedReader(stream, len)
            } else if headers.has::<ContentLength>() {
                trace!("illegal Content-Length: {:?}", headers.get_raw("Content-Length"));
                return Err(Error::Header);
            } else {
                trace!("neither Transfer-Encoding nor Content-Length");
                EofReader(stream)
            }
        });

        trace!("Http11Message.reader = {:?}", self.reader);


        Ok(ResponseHead {
            headers: headers,
            raw_status: raw_status,
            version: head.version,
        })
        */
    }
}


*/


