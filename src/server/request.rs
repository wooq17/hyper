//! Server Requests
//!
//! These are requests that a `hyper::Server` receives, and include its method,
//! target URI, headers, and message body.
use std::io::{Read, Cursor};
use std::net::SocketAddr;

use version::{HttpVersion};
use method::Method::{self, Get, Head};
use header::{Headers, ContentLength, TransferEncoding};
use http::{IncomingRequest, Incoming};
//use http::h1::HttpReader::{SizedReader, ChunkedReader, EmptyReader};
use uri::RequestUri;

/// A request bundles several parts of an incoming `NetworkStream`, given to a `Handler`.
#[derive(Debug)]
pub struct Request {
    /// The IP address of the remote connection.
    //pub remote_addr: SocketAddr,
    /// The `Method`, such as `Get`, `Post`, etc.
    pub method: Method,
    /// The headers of the incoming request.
    pub headers: Headers,
    /// The target request-uri for this request.
    pub uri: RequestUri,
    /// The version of HTTP for this request.
    pub version: HttpVersion,

    //stream: Option<::eventual::Stream<Vec<u8>, ::Error>>
}


impl Request {
    /// Create a new Request, reading the StartLine and Headers so they are
    /// immediately useful.
    pub fn new(incoming: IncomingRequest) -> Request {
        let Incoming { version, subject: (method, uri), headers } = incoming;
        debug!("Request Line: {:?} {:?} {:?}", method, uri, version);
        debug!("{:#?}", headers);

        /*
        let body = if method == Get || method == Head {
            EmptyReader(buf)
        } else if let Some(&ContentLength(len)) = headers.get() {
            SizedReader(buf, len)
        } else if headers.has::<TransferEncoding>() {
            todo!("check for Transfer-Encoding: chunked");
            ChunkedReader(buf, None)
        } else {
            EmptyReader(buf)
        };
        */

        Request {
            //remote_addr: addr,
            method: method,
            uri: uri,
            headers: headers,
            version: version,
        }
    }

    /*
    pub fn read(mut self) -> Future<(Vec<u8>, Request), (::Error, Request)> {
        let (complete, future) = Future::pair();
        self.stream.take().expect("Request Stream lost").receive(move |res| {
            match res {
                Ok(Some((vec, stream))) => {
                    self.stream = Some(stream);
                    complete.complete((vec, self))
                },
                Ok(None) => panic!("stream is empty?"),
                Err(e) => if let Some(e) = e.take() {
                    complete.fail((e, self))
                }
            }
        });
        future
    }


    pub fn stream(mut self) -> ::eventual::Stream<Vec<u8>, ::Error> {
        self.stream.take().expect("Request Stream lost")
    }
    */

    /*
    /// Get a reference to the underlying `NetworkStream`.
    #[inline]
    pub fn downcast_ref<T: Read>(&self) -> Option<&T> {
        self.body.get_ref().downcast_ref()
    }

    /// Get a reference to the underlying Ssl stream, if connected
    /// over HTTPS.
    ///
    /// # Example
    ///
    /// ```rust
    /// # extern crate hyper;
    /// # #[cfg(feature = "openssl")]
    /// extern crate openssl;
    /// # #[cfg(feature = "openssl")]
    /// use openssl::ssl::SslStream;
    /// use hyper::net::HttpStream;
    /// # fn main() {}
    /// # #[cfg(feature = "openssl")]
    /// # fn doc_ssl(req: hyper::server::Request) {
    /// let maybe_ssl = req.ssl::<SslStream<HttpStream>>();
    /// # }
    /// ```
    #[inline]
    pub fn ssl<T: Read>(&self) -> Option<&T> {
        use ::net::HttpsStream;
        match self.downcast_ref() {
            Some(&HttpsStream::Https(ref s)) => Some(s),
            _ => None
        }
    }

    /// Deconstruct a Request into its constituent parts.
    #[inline]
    pub fn deconstruct(self) -> (SocketAddr, Method, Headers,
                                 RequestUri, HttpVersion,
                                 HttpReader<&'s mut NetworkRead>) {
        (self.remote_addr, self.method, self.headers,
         self.uri, self.version, self.body)
    }
    */
}

fn read_buf<R: Read>(buf: &mut R, len: usize) -> ::Result<Vec<u8>> {
    let mut v = vec![0; len];
    let count = try!(buf.read(&mut v));
    v.truncate(count);
    Ok(v)
}

#[cfg(test)]
mod tests {
    use buffer::BufReader;
    use header::{Host, TransferEncoding, Encoding};
    use net::NetworkStream;
    use mock::MockStream;
    use super::Request;

    use std::io::{self, Read};
    use std::net::SocketAddr;

    fn sock(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    fn read_to_string(mut req: Request) -> io::Result<String> {
        let mut s = String::new();
        try!(req.read_to_string(&mut s));
        Ok(s)
    }

    #[test]
    fn test_get_empty_body() {
        let mut mock = MockStream::with_input(b"\
            GET / HTTP/1.1\r\n\
            Host: example.domain\r\n\
            \r\n\
            I'm a bad request.\r\n\
        ");

        // FIXME: Use Type ascription
        let mock: &mut NetworkStream = &mut mock;
        let mut stream = BufReader::new(mock);

        let req = Request::new(&mut stream, sock("127.0.0.1:80")).unwrap();
        assert_eq!(read_to_string(req).unwrap(), "".to_owned());
    }

    #[test]
    fn test_head_empty_body() {
        let mut mock = MockStream::with_input(b"\
            HEAD / HTTP/1.1\r\n\
            Host: example.domain\r\n\
            \r\n\
            I'm a bad request.\r\n\
        ");

        // FIXME: Use Type ascription
        let mock: &mut NetworkStream = &mut mock;
        let mut stream = BufReader::new(mock);

        let req = Request::new(&mut stream, sock("127.0.0.1:80")).unwrap();
        assert_eq!(read_to_string(req).unwrap(), "".to_owned());
    }

    #[test]
    fn test_post_empty_body() {
        let mut mock = MockStream::with_input(b"\
            POST / HTTP/1.1\r\n\
            Host: example.domain\r\n\
            \r\n\
            I'm a bad request.\r\n\
        ");

        // FIXME: Use Type ascription
        let mock: &mut NetworkStream = &mut mock;
        let mut stream = BufReader::new(mock);

        let req = Request::new(&mut stream, sock("127.0.0.1:80")).unwrap();
        assert_eq!(read_to_string(req).unwrap(), "".to_owned());
    }

    /*
    #[test]
    fn test_parse_chunked_request() {
        let mut mock = MockStream::with_input(b"\
            POST / HTTP/1.1\r\n\
            Host: example.domain\r\n\
            Transfer-Encoding: chunked\r\n\
            \r\n\
            1\r\n\
            q\r\n\
            2\r\n\
            we\r\n\
            2\r\n\
            rt\r\n\
            0\r\n\
            \r\n"
        );

        // FIXME: Use Type ascription
        let mock: &mut NetworkStream = &mut mock;
        let mut stream = BufReader::new(mock);

        let req = Request::new(&mut stream, sock("127.0.0.1:80")).unwrap();

        // The headers are correct?
        match req.headers.get::<Host>() {
            Some(host) => {
                assert_eq!("example.domain", host.hostname);
            },
            None => panic!("Host header expected!"),
        };
        match req.headers.get::<TransferEncoding>() {
            Some(encodings) => {
                assert_eq!(1, encodings.len());
                assert_eq!(Encoding::Chunked, encodings[0]);
            }
            None => panic!("Transfer-Encoding: chunked expected!"),
        };
        // The content is correctly read?
        assert_eq!(read_to_string(req).unwrap(), "qwert".to_owned());
    }

    /// Tests that when a chunk size is not a valid radix-16 number, an error
    /// is returned.
    #[test]
    fn test_invalid_chunk_size_not_hex_digit() {
        let mut mock = MockStream::with_input(b"\
            POST / HTTP/1.1\r\n\
            Host: example.domain\r\n\
            Transfer-Encoding: chunked\r\n\
            \r\n\
            X\r\n\
            1\r\n\
            0\r\n\
            \r\n"
        );

        // FIXME: Use Type ascription
        let mock: &mut NetworkStream = &mut mock;
        let mut stream = BufReader::new(mock);

        let req = Request::new(&mut stream, sock("127.0.0.1:80")).unwrap();

        assert!(read_to_string(req).is_err());
    }

    /// Tests that when a chunk size contains an invalid extension, an error is
    /// returned.
    #[test]
    fn test_invalid_chunk_size_extension() {
        let mut mock = MockStream::with_input(b"\
            POST / HTTP/1.1\r\n\
            Host: example.domain\r\n\
            Transfer-Encoding: chunked\r\n\
            \r\n\
            1 this is an invalid extension\r\n\
            1\r\n\
            0\r\n\
            \r\n"
        );

        // FIXME: Use Type ascription
        let mock: &mut NetworkStream = &mut mock;
        let mut stream = BufReader::new(mock);

        let req = Request::new(&mut stream, sock("127.0.0.1:80")).unwrap();

        assert!(read_to_string(req).is_err());
    }

    /// Tests that when a valid extension that contains a digit is appended to
    /// the chunk size, the chunk is correctly read.
    #[test]
    fn test_chunk_size_with_extension() {
        let mut mock = MockStream::with_input(b"\
            POST / HTTP/1.1\r\n\
            Host: example.domain\r\n\
            Transfer-Encoding: chunked\r\n\
            \r\n\
            1;this is an extension with a digit 1\r\n\
            1\r\n\
            0\r\n\
            \r\n"
        );

        // FIXME: Use Type ascription
        let mock: &mut NetworkStream = &mut mock;
        let mut stream = BufReader::new(mock);

        let req = Request::new(&mut stream, sock("127.0.0.1:80")).unwrap();

        assert_eq!(read_to_string(req).unwrap(), "1".to_owned());
    }*/

}
