use std::cmp;
use std::fmt;
use std::io::{self, Read};

use self::HttpReader::{SizedReader, ChunkedReader, EofReader, EmptyReader};

/// Readers to handle different Transfer-Encodings.
///
/// If a message body does not include a Transfer-Encoding, it *should*
/// include a Content-Length header.
pub enum HttpReader<R> {
    /// A Reader used when a Content-Length header is passed with a positive integer.
    SizedReader(R, u64),
    /// A Reader used when Transfer-Encoding is `chunked`.
    ChunkedReader(R, Option<u64>),
    /// A Reader used for responses that don't indicate a length or chunked.
    ///
    /// Note: This should only used for `Response`s. It is illegal for a
    /// `Request` to be made with both `Content-Length` and
    /// `Transfer-Encoding: chunked` missing, as explained from the spec:
    ///
    /// > If a Transfer-Encoding header field is present in a response and
    /// > the chunked transfer coding is not the final encoding, the
    /// > message body length is determined by reading the connection until
    /// > it is closed by the server.  If a Transfer-Encoding header field
    /// > is present in a request and the chunked transfer coding is not
    /// > the final encoding, the message body length cannot be determined
    /// > reliably; the server MUST respond with the 400 (Bad Request)
    /// > status code and then close the connection.
    EofReader(R),
    /// A Reader used for messages that should never have a body.
    ///
    /// See https://tools.ietf.org/html/rfc7230#section-3.3.3
    EmptyReader(R),
}

impl<R: Read> HttpReader<R> {

    /// Unwraps this HttpReader and returns the underlying Reader.
    pub fn into_inner(self) -> R {
        match self {
            SizedReader(r, _) => r,
            ChunkedReader(r, _) => r,
            EofReader(r) => r,
            EmptyReader(r) => r,
        }
    }

    /// Gets a borrowed reference to the underlying Reader.
    pub fn get_ref(&self) -> &R {
        match *self {
            SizedReader(ref r, _) => r,
            ChunkedReader(ref r, _) => r,
            EofReader(ref r) => r,
            EmptyReader(ref r) => r,
        }
    }

    /// Gets a mutable reference to the underlying Reader.
    pub fn get_mut(&mut self) -> &mut R {
        match *self {
            SizedReader(ref mut r, _) => r,
            ChunkedReader(ref mut r, _) => r,
            EofReader(ref mut r) => r,
            EmptyReader(ref mut r) => r,
        }
    }

    pub fn has_body(&self) -> bool {
        match *self {
            EmptyReader(..) |
            SizedReader(_, 0) |
            ChunkedReader(_, Some(0)) => false,
            // specifically EofReader is always true
            _ => true
        }
    }
}

impl<R> fmt::Debug for HttpReader<R> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            SizedReader(_,rem) => write!(fmt, "SizedReader(remaining={:?})", rem),
            ChunkedReader(_, None) => write!(fmt, "ChunkedReader(chunk_remaining=unknown)"),
            ChunkedReader(_, Some(rem)) => write!(fmt, "ChunkedReader(chunk_remaining={:?})", rem),
            EofReader(_) => write!(fmt, "EofReader"),
            EmptyReader(_) => write!(fmt, "EmptyReader"),
        }
    }
}

impl<R: Read> Read for HttpReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match *self {
            SizedReader(ref mut body, ref mut remaining) => {
                trace!("Sized read, remaining={:?}", remaining);
                if *remaining == 0 {
                    Ok(0)
                } else {
                    let to_read = cmp::min(*remaining as usize, buf.len());
                    let num = try!(body.read(&mut buf[..to_read])) as u64;
                    trace!("Sized read: {}", num);
                    if num > *remaining {
                        *remaining = 0;
                    } else if num == 0 {
                        return Err(io::Error::new(io::ErrorKind::Other, "early eof"));
                    } else {
                        *remaining -= num;
                    }
                    Ok(num as usize)
                }
            },
            ChunkedReader(ref mut body, ref mut opt_remaining) => {
                let mut rem = match *opt_remaining {
                    Some(ref rem) => *rem,
                    // None means we don't know the size of the next chunk
                    None => try!(read_chunk_size(body))
                };
                trace!("Chunked read, remaining={:?}", rem);

                if rem == 0 {
                    *opt_remaining = Some(0);

                    // chunk of size 0 signals the end of the chunked stream
                    // if the 0 digit was missing from the stream, it would
                    // be an InvalidInput error instead.
                    trace!("end of chunked");
                    return Ok(0)
                }

                let to_read = cmp::min(rem as usize, buf.len());
                let count = try!(body.read(&mut buf[..to_read])) as u64;

                if count == 0 {
                    *opt_remaining = Some(0);
                    return Err(io::Error::new(io::ErrorKind::Other, "early eof"));
                }

                rem -= count;
                *opt_remaining = if rem > 0 {
                    Some(rem)
                } else {
                    try!(eat(body, b"\r\n"));
                    None
                };
                Ok(count as usize)
            },
            EofReader(ref mut body) => {
                let r = body.read(buf);
                trace!("eofread: {:?}", r);
                r
            },
            EmptyReader(_) => Ok(0)
        }
    }
}

fn eat<R: Read>(rdr: &mut R, bytes: &[u8]) -> io::Result<()> {
    let mut buf = [0];
    for &b in bytes.iter() {
        match try!(rdr.read(&mut buf)) {
            1 if buf[0] == b => (),
            _ => return Err(io::Error::new(io::ErrorKind::InvalidInput,
                                          "Invalid characters found")),
        }
    }
    Ok(())
}

/// Chunked chunks start with 1*HEXDIGIT, indicating the size of the chunk.
fn read_chunk_size<R: Read>(rdr: &mut R) -> io::Result<u64> {
    macro_rules! byte (
        ($rdr:ident) => ({
            let mut buf = [0];
            match try!($rdr.read(&mut buf)) {
                1 => buf[0],
                _ => return Err(io::Error::new(io::ErrorKind::InvalidInput,
                                                  "Invalid chunk size line")),

            }
        })
    );
    let mut size = 0u64;
    let radix = 16;
    let mut in_ext = false;
    let mut in_chunk_size = true;
    loop {
        match byte!(rdr) {
            b@b'0'...b'9' if in_chunk_size => {
                size *= radix;
                size += (b - b'0') as u64;
            },
            b@b'a'...b'f' if in_chunk_size => {
                size *= radix;
                size += (b + 10 - b'a') as u64;
            },
            b@b'A'...b'F' if in_chunk_size => {
                size *= radix;
                size += (b + 10 - b'A') as u64;
            },
            b'\r' => {
                match byte!(rdr) {
                    b'\n' => break,
                    _ => return Err(io::Error::new(io::ErrorKind::InvalidInput,
                                                  "Invalid chunk size line"))

                }
            },
            // If we weren't in the extension yet, the ";" signals its start
            b';' if !in_ext => {
                in_ext = true;
                in_chunk_size = false;
            },
            // "Linear white space" is ignored between the chunk size and the
            // extension separator token (";") due to the "implied *LWS rule".
            b'\t' | b' ' if !in_ext & !in_chunk_size => {},
            // LWS can follow the chunk size, but no more digits can come
            b'\t' | b' ' if in_chunk_size => in_chunk_size = false,
            // We allow any arbitrary octet once we are in the extension, since
            // they all get ignored anyway. According to the HTTP spec, valid
            // extensions would have a more strict syntax:
            //     (token ["=" (token | quoted-string)])
            // but we gain nothing by rejecting an otherwise valid chunk size.
            ext if in_ext => {
                todo!("chunk extension byte={}", ext);
            },
            // Finally, if we aren't in the extension and we're reading any
            // other octet, the chunk size line is invalid!
            _ => {
                return Err(io::Error::new(io::ErrorKind::InvalidInput,
                                         "Invalid chunk size line"));
            }
        }
    }
    trace!("chunk size={:?}", size);
    Ok(size)
}


#[cfg(test)]
mod tests {

    #[test]
    fn test_read_chunk_size() {
        fn read(s: &str, result: u64) {
            assert_eq!(read_chunk_size(&mut s.as_bytes()).unwrap(), result);
        }

        fn read_err(s: &str) {
            assert_eq!(read_chunk_size(&mut s.as_bytes()).unwrap_err().kind(),
                io::ErrorKind::InvalidInput);
        }

        read("1\r\n", 1);
        read("01\r\n", 1);
        read("0\r\n", 0);
        read("00\r\n", 0);
        read("A\r\n", 10);
        read("a\r\n", 10);
        read("Ff\r\n", 255);
        read("Ff   \r\n", 255);
        // Missing LF or CRLF
        read_err("F\rF");
        read_err("F");
        // Invalid hex digit
        read_err("X\r\n");
        read_err("1X\r\n");
        read_err("-\r\n");
        read_err("-1\r\n");
        // Acceptable (if not fully valid) extensions do not influence the size
        read("1;extension\r\n", 1);
        read("a;ext name=value\r\n", 10);
        read("1;extension;extension2\r\n", 1);
        read("1;;;  ;\r\n", 1);
        read("2; extension...\r\n", 2);
        read("3   ; extension=123\r\n", 3);
        read("3   ;\r\n", 3);
        read("3   ;   \r\n", 3);
        // Invalid extensions cause an error
        read_err("1 invalid extension\r\n");
        read_err("1 A\r\n");
        read_err("1;no CRLF");
    }

    #[test]
    fn test_read_sized_early_eof() {
        let mut r = super::HttpReader::SizedReader(MockStream::with_input(b"foo bar"), 10);
        let mut buf = [0u8; 10];
        assert_eq!(r.read(&mut buf).unwrap(), 7);
        let e = r.read(&mut buf).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::Other);
        assert_eq!(e.description(), "early eof");
    }

    #[test]
    fn test_read_chunked_early_eof() {
        let mut r = super::HttpReader::ChunkedReader(MockStream::with_input(b"\
            9\r\n\
            foo bar\
        "), None);

        let mut buf = [0u8; 10];
        assert_eq!(r.read(&mut buf).unwrap(), 7);
        let e = r.read(&mut buf).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::Other);
        assert_eq!(e.description(), "early eof");
    }

 
}
