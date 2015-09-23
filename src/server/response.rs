//! Server Responses
//!
//! These are responses sent by a `hyper::Server` to clients, after
//! receiving a request.
use std::any::{Any, TypeId};
use std::mem;
use std::io::{Write, BufWriter};
use std::ptr;

use time::now_utc;

use header;
use http;
use status;
use net::{Fresh, Streaming};
use version;


/// The outgoing half for a Tcp connection, created by a `Server` and given to a `Handler`.
///
/// The default `StatusCode` for a `Response` is `200 OK`.
///
/// There is a `Drop` implementation for `Response` that will automatically
/// write the head and flush the body, if the handler has not already done so,
/// so that the server doesn't accidentally leave dangling requests.
#[derive(Debug)]
pub struct Response<W: Any = Fresh> {
    /// The HTTP version of this response.
    pub version: version::HttpVersion,
    // The status code for the request.
    status: status::StatusCode,
    // The outgoing headers on this response.
    headers: header::Headers,

    body: http::Transfer<http::Response, W>,
}

impl<W: Any> Response<W> {
    /// The status of this response.
    #[inline]
    pub fn status(&self) -> status::StatusCode { self.status }

    /// The headers of this response.
    #[inline]
    pub fn headers(&self) -> &header::Headers { &self.headers }

    /*
    /// Construct a Response from its constituent parts.
    #[inline]
    pub fn construct(version: version::HttpVersion,
                     body: HttpWriter<&'a mut (Write + 'a)>,
                     status: status::StatusCode,
                     headers: &'a mut header::Headers) -> Response<'a, Fresh> {
        Response {
            status: status,
            version: version,
            body: body,
            headers: headers,
        }
    }
    */

    /// Deconstruct this Response into its constituent parts.
    #[inline]
    pub fn deconstruct(self) -> (version::HttpVersion, http::Transfer<http::Response, W>,
                                 status::StatusCode, header::Headers) {
        unsafe {
            let parts = (
                self.version,
                ptr::read(&self.body),
                self.status,
                ptr::read(&self.headers)
            );
            mem::forget(self);
            parts
        }
    }
}


impl Response<Fresh> {
    /// Creates a new Response that can be used to write to a network stream.
    #[inline]
    pub fn new(tx: http::Transfer<http::Response, Fresh>) -> Response<Fresh> {
        Response {
            status: status::StatusCode::Ok,
            version: version::HttpVersion::Http11,
            headers: header::Headers::new(),
            body: tx,
        }
    }

    /// Writes the body and ends the response.
    ///
    /// This is a shortcut method for when you have a response with a fixed
    /// size, and would only need a single `write` call normally.
    ///
    /// # Example
    ///
    /// ```
    /// # use hyper::server::Response;
    /// fn handler(res: Response) {
    ///     res.send(b"Hello World!")
    /// }
    /// ```
    ///
    /// The above is a short for this longer form:
    ///
    /// ```
    /// # use hyper::server::Response;
    /// use std::io::Write;
    /// use hyper::header::ContentLength;
    /// fn handler(mut res: Response) {
    ///     let body = b"Hello World!";
    ///     res.headers_mut().set(ContentLength(body.len() as u64));
    ///     res.start().write(body);
    /// }
    /// ```
    #[inline]
    pub fn send(mut self, data: &[u8]) {
        self.headers.set(header::ContentLength(data.len() as u64));
        let mut streaming = self.start();
        streaming.write(data)
    }

    /// Consume this Response<Fresh>, writing the Headers and Status and
    /// creating a Response<Streaming>
    pub fn start(mut self) -> Response<Streaming> {
        let (version, body, status, mut headers) = self.deconstruct();
        let body = body.start(version, status, &mut headers);
        Response {
            version: version,
            status: status,
            headers: headers,
            body: body
        }
    }

    /// Get a mutable reference to the status.
    #[inline]
    pub fn status_mut(&mut self) -> &mut status::StatusCode { &mut self.status }

    /// Get a mutable reference to the Headers.
    #[inline]
    pub fn headers_mut(&mut self) -> &mut header::Headers { &mut self.headers }
}


impl Response<Streaming> {
    /// Asynchronously write bytes to the response.
    #[inline]
    pub fn write(&mut self, data: &[u8]) {
        self.body.write(data)
    }

    /// Asynchonously flushes all writing of a response to the client.
    #[inline]
    pub fn end(self) {
        // dropped
    }
}


impl<T: Any> Drop for Response<T> {
    fn drop(&mut self) {
        if TypeId::of::<T>() == TypeId::of::<Fresh>() {
            let res = unsafe { ptr::read(self) };
            mem::forget(self);
            let (version, body, status, mut headers) = res.deconstruct();
            headers.set(header::ContentLength(0));
            let body = unsafe {
                mem::transmute::<_, http::Transfer<http::Response, Fresh>>(body)
            };
            body.start(version, status, &mut headers);
        };

        /* TODO: this should happen in http::Transfer
        // AsyncWriter will flush on drop
        if !http::should_keep_alive(self.version, &self.headers) {
            trace!("not keep alive, closing");
            self.body.get_mut().get_mut().get_mut().close();
        }
        */
    }
}

#[cfg(test)]
mod tests {
    use header::Headers;
    use mock::MockStream;
    use super::Response;

    macro_rules! lines {
        ($s:ident = $($line:pat),+) => ({
            let s = String::from_utf8($s.write).unwrap();
            let mut lines = s.split_terminator("\r\n");

            $(
                match lines.next() {
                    Some($line) => (),
                    other => panic!("line mismatch: {:?} != {:?}", other, stringify!($line))
                }
            )+

            assert_eq!(lines.next(), None);
        })
    }

    #[test]
    fn test_fresh_start() {
        let mut headers = Headers::new();
        let mut stream = MockStream::new();
        {
            let res = Response::new(&mut stream, &mut headers);
            res.start().unwrap().deconstruct();
        }

        lines! { stream =
            "HTTP/1.1 200 OK",
            _date,
            _transfer_encoding,
            ""
        }
    }

    #[test]
    fn test_streaming_end() {
        let mut headers = Headers::new();
        let mut stream = MockStream::new();
        {
            let res = Response::new(&mut stream, &mut headers);
            res.start().unwrap().end().unwrap();
        }

        lines! { stream =
            "HTTP/1.1 200 OK",
            _date,
            _transfer_encoding,
            "",
            "0",
            "" // empty zero body
        }
    }

    #[test]
    fn test_fresh_drop() {
        use status::StatusCode;
        let mut headers = Headers::new();
        let mut stream = MockStream::new();
        {
            let mut res = Response::new(&mut stream, &mut headers);
            *res.status_mut() = StatusCode::NotFound;
        }

        lines! { stream =
            "HTTP/1.1 404 Not Found",
            _date,
            _transfer_encoding,
            "",
            "0",
            "" // empty zero body
        }
    }

    #[test]
    fn test_streaming_drop() {
        use std::io::Write;
        use status::StatusCode;
        let mut headers = Headers::new();
        let mut stream = MockStream::new();
        {
            let mut res = Response::new(&mut stream, &mut headers);
            *res.status_mut() = StatusCode::NotFound;
            let mut stream = res.start().unwrap();
            stream.write_all(b"foo").unwrap();
        }

        lines! { stream =
            "HTTP/1.1 404 Not Found",
            _date,
            _transfer_encoding,
            "",
            "3",
            "foo",
            "0",
            "" // empty zero body
        }
    }
}
