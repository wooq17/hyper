use std::borrow::Cow;

use httparse;

use error::Error;
use header::Headers;
use http::{Incoming, RawStatus, Parse, ParseResult};
use method::Method;
use status::StatusCode;
use uri::RequestUri;
use version::HttpVersion::{Http10, Http11};

const MAX_HEADERS: usize = 100;

/// Parses a request into an Incoming message head.
#[inline]
pub fn parse_request(buf: &[u8]) -> ParseResult<(Method, RequestUri)> {
    parse::<httparse::Request, (Method, RequestUri)>(buf)
}

/// Parses a response into an Incoming message head.
#[inline]
pub fn parse_response(buf: &[u8]) -> ParseResult<RawStatus> {
    parse::<httparse::Response, RawStatus>(buf)
}

pub fn parse<T: Parse<Subject=I>, I>(buf: &[u8]) -> ParseResult<I> {
    if buf.len() == 0 {
        return Ok(None);
    }
    trace!("parse({:?})", buf);
    <T as Parse>::parse(buf)
}



impl<'a> Parse for httparse::Request<'a, 'a> {
    type Subject = (Method, RequestUri);

    fn parse(buf: &[u8]) -> ParseResult<(Method, RequestUri)> {
        let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
        trace!("Request.parse([Header; {}], [u8; {}])", headers.len(), buf.len());
        let mut req = httparse::Request::new(&mut headers);
        Ok(match try!(req.parse(buf)) {
            httparse::Status::Complete(len) => {
                trace!("Request.parse Complete({})", len);
                Some((Incoming {
                    version: if req.version.unwrap() == 1 { Http11 } else { Http10 },
                    subject: (
                        try!(req.method.unwrap().parse()),
                        try!(req.path.unwrap().parse())
                    ),
                    headers: try!(Headers::from_raw(req.headers))
                }, len))
            },
            httparse::Status::Partial => None
        })
    }
}

impl<'a> Parse for httparse::Response<'a, 'a> {
    type Subject = RawStatus;

    fn parse(buf: &[u8]) -> ParseResult<RawStatus> {
        let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
        trace!("Response.parse([Header; {}], [u8; {}])", headers.len(), buf.len());
        let mut res = httparse::Response::new(&mut headers);
        Ok(match try!(res.parse(buf)) {
            httparse::Status::Complete(len) => {
                trace!("Response.try_parse Complete({})", len);
                let code = res.code.unwrap();
                let reason = match StatusCode::from_u16(code).canonical_reason() {
                    Some(reason) if reason == res.reason.unwrap() => Cow::Borrowed(reason),
                    _ => Cow::Owned(res.reason.unwrap().to_owned())
                };
                Some((Incoming {
                    version: if res.version.unwrap() == 1 { Http11 } else { Http10 },
                    subject: RawStatus(code, reason),
                    headers: try!(Headers::from_raw(res.headers))
                }, len))
            },
            httparse::Status::Partial => None
        })
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_parse_incoming() {
        let mut raw = MockStream::with_input(b"GET /echo HTTP/1.1\r\nHost: hyper.rs\r\n\r\n");
        let mut buf = BufReader::new(&mut raw);
        parse_request(&mut buf).unwrap();
    }

    #[test]
    fn test_parse_raw_status() {
        let mut raw = MockStream::with_input(b"HTTP/1.1 200 OK\r\n\r\n");
        let mut buf = BufReader::new(&mut raw);
        let res = parse_response(&mut buf).unwrap();

        assert_eq!(res.subject.1, "OK");

        let mut raw = MockStream::with_input(b"HTTP/1.1 200 Howdy\r\n\r\n");
        let mut buf = BufReader::new(&mut raw);
        let res = parse_response(&mut buf).unwrap();

        assert_eq!(res.subject.1, "Howdy");
    }


    #[test]
    fn test_parse_tcp_closed() {
        use std::io::ErrorKind;
        use error::Error;

        let mut empty = MockStream::new();
        let mut buf = BufReader::new(&mut empty);
        match parse_request(&mut buf) {
            Err(Error::Io(ref e)) if e.kind() == ErrorKind::ConnectionAborted => (),
            other => panic!("unexpected result: {:?}", other)
        }
    }

    #[cfg(feature = "nightly")]
    use test::Bencher;

    #[cfg(feature = "nightly")]
    #[bench]
    fn bench_parse_incoming(b: &mut Bencher) {
        let mut raw = MockStream::with_input(b"GET /echo HTTP/1.1\r\nHost: hyper.rs\r\n\r\n");
        let mut buf = BufReader::new(&mut raw);
        b.iter(|| {
            parse_request(&mut buf).unwrap();
            buf.get_mut().read.set_position(0);
        });
    }

}
