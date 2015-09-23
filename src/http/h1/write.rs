use std::fmt;
use std::io::{self, Write};
use std::mem;
use std::ptr;

use self::HttpWriter::{ChunkedWriter, SizedWriter, EmptyWriter, ThroughWriter};

/// Writers to handle different Transfer-Encodings.
pub enum HttpWriter<W: Write> {
    /// A pass-through Writer, used initially before Transfer-Encoding is determined.
    ThroughWriter(W),
    /// A Writer for when Transfer-Encoding includes `chunked`.
    ChunkedWriter(W),
    /// A Writer for when Content-Length is set.
    ///
    /// Enforces that the body is not longer than the Content-Length header.
    SizedWriter(W, u64),
    /// A writer that should not write any body.
    EmptyWriter(W),
}

impl<W: Write> HttpWriter<W> {
    /// Unwraps the HttpWriter and returns the underlying Writer.
    #[inline]
    pub fn into_inner(self) -> W {
        unsafe {
            let w = ptr::read(match self {
                ThroughWriter(ref w) => w,
                ChunkedWriter(ref w) => w,
                SizedWriter(ref w, _) => w,
                EmptyWriter(ref w) => w,
            });
            mem::forget(self);
            w
        }
    }

    /// Access the inner Writer.
    #[inline]
    pub fn get_ref<'a>(&'a self) -> &'a W {
        match *self {
            ThroughWriter(ref w) => w,
            ChunkedWriter(ref w) => w,
            SizedWriter(ref w, _) => w,
            EmptyWriter(ref w) => w,
        }
    }

    /// Access the inner Writer mutably.
    ///
    /// Warning: You should not write to this directly, as you can corrupt
    /// the state.
    #[inline]
    pub fn get_mut<'a>(&'a mut self) -> &'a mut W {
        match *self {
            ThroughWriter(ref mut w) => w,
            ChunkedWriter(ref mut w) => w,
            SizedWriter(ref mut w, _) => w,
            EmptyWriter(ref mut w) => w,
        }
    }

    /// Ends the HttpWriter, and returns the underlying Writer.
    ///
    /// A final `write_all()` is called with an empty message, and then flushed.
    /// The ChunkedWriter variant will use this to write the 0-sized last-chunk.
    #[inline]
    pub fn end(mut self) -> Result<W, EndError<W>> {
        fn inner<W: Write>(w: &mut W) -> io::Result<()> {
            try!(w.write(&[]));
            w.flush()
        }

        match inner(&mut self) {
            Ok(..) => Ok(self.into_inner()),
            Err(e) => Err(EndError(e, self))
        }
    }
}

#[derive(Debug)]
pub struct EndError<W: Write>(io::Error, HttpWriter<W>);

impl<W: Write> From<EndError<W>> for io::Error {
    fn from(e: EndError<W>) -> io::Error {
        e.0
    }
}

impl<W: Write> Write for HttpWriter<W> {
    #[inline]
    fn write(&mut self, msg: &[u8]) -> io::Result<usize> {
        match *self {
            ThroughWriter(ref mut w) => w.write(msg),
            ChunkedWriter(ref mut w) => {
                let chunk_size = msg.len();
                trace!("chunked write, size = {:?}", chunk_size);
                try!(write!(w, "{:X}\r\n", chunk_size));
                try!(w.write_all(msg));
                try!(w.write_all(b"\r\n"));
                Ok(msg.len())
            },
            SizedWriter(ref mut w, ref mut remaining) => {
                let len = msg.len() as u64;
                if len > *remaining {
                    let len = *remaining;
                    *remaining = 0;
                    try!(w.write_all(&msg[..len as usize]));
                    Ok(len as usize)
                } else {
                    *remaining -= len;
                    try!(w.write_all(msg));
                    Ok(len as usize)
                }
            },
            EmptyWriter(..) => {
                if !msg.is_empty() {
                    error!("Cannot include a body with this kind of message");
                }
                Ok(0)
            }
        }
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        match *self {
            ThroughWriter(ref mut w) => w.flush(),
            ChunkedWriter(ref mut w) => w.flush(),
            SizedWriter(ref mut w, _) => w.flush(),
            EmptyWriter(ref mut w) => w.flush(),
        }
    }
}

impl<W: Write> Drop for HttpWriter<W> {
    fn drop(&mut self) {
        match *self {
            ChunkedWriter(..) => {
                if let Err(e) = self.write(&[]) {
                    error!("error writing end chunk: {}", e);
                }
            },
            _ => ()
        }
    }
}

impl<W: Write> fmt::Debug for HttpWriter<W> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ThroughWriter(_) => write!(fmt, "ThroughWriter"),
            ChunkedWriter(_) => write!(fmt, "ChunkedWriter"),
            SizedWriter(_, rem) => write!(fmt, "SizedWriter(remaining={:?})", rem),
            EmptyWriter(_) => write!(fmt, "EmptyWriter"),
        }
    }
}


#[cfg(test)]
mod tests {
    #[test]
    fn test_write_chunked() {
        use std::str::from_utf8;
        let mut w = super::HttpWriter::ChunkedWriter(Vec::new());
        w.write_all(b"foo bar").unwrap();
        w.write_all(b"baz quux herp").unwrap();
        let buf = w.end().unwrap();
        let s = from_utf8(buf.as_ref()).unwrap();
        assert_eq!(s, "7\r\nfoo bar\r\nD\r\nbaz quux herp\r\n0\r\n\r\n");
    }

    #[test]
    fn test_write_sized() {
        use std::str::from_utf8;
        let mut w = super::HttpWriter::SizedWriter(Vec::new(), 8);
        w.write_all(b"foo bar").unwrap();
        assert_eq!(w.write(b"baz").unwrap(), 1);

        let buf = w.end().unwrap();
        let s = from_utf8(buf.as_ref()).unwrap();
        assert_eq!(s, "foo barb");
    }


}
