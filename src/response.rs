use std::io::{self, Read, Write};

use headers::{HeaderMapExt, HeaderValue};

use crate::body::{Body, BodyInner};

enum Encoding {
    FixedLength(usize),
    Chunked,
    CloseDelimited,
}

pub(crate) fn write_response(res: http::Response<Body>, stream: &mut impl Write) -> io::Result<()> {
    let (parts, body) = res.into_parts();

    let mut headers = parts.headers;

    let te = headers.typed_get::<headers::TransferEncoding>();
    let connection = headers.typed_get::<headers::Connection>();

    let has_connection_close = connection.map(|conn| conn.contains("close")) == Some(true);
    let has_chunked_encoding = te.map(|te| te.is_chunked()) == Some(true);
    let content_length = headers.typed_get::<headers::ContentLength>();

    let encoding = match (
        has_connection_close,
        has_chunked_encoding,
        content_length,
        &body.0,
    ) {
        (_, _, Some(_), BodyInner::Empty) => {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "content-length doesn't match body length",
            ));
        }

        (_, _, None, BodyInner::Empty) => None,

        (_, false, _, BodyInner::Chunked(_)) => {
            headers.remove("content-length");
            headers.insert("transfer-encoding", HeaderValue::from_static("chunked"));
            Some(Encoding::Chunked)
        }

        (_, true, _, _) => {
            headers.remove("content-length");
            Some(Encoding::Chunked)
        }

        (_, false, Some(len), BodyInner::Buffered(ref buf)) if buf.len() != len.0 as usize => {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "content-length doesn't match body length",
            ));
        }

        (_, false, Some(len), BodyInner::Reader(_, Some(body_len)))
            if len.0 as usize != *body_len =>
        {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "content-length doesn't match body length",
            ));
        }

        (_, false, Some(len), _) => Some(Encoding::FixedLength(len.0 as usize)),

        (true, false, None, _) => Some(Encoding::CloseDelimited),

        (false, false, None, BodyInner::Buffered(ref buf)) => {
            let len: u64 = buf.len().try_into().unwrap();
            headers.typed_insert::<headers::ContentLength>(headers::ContentLength(len));
            Some(Encoding::FixedLength(len as usize))
        }

        (false, false, None, BodyInner::Reader(_, Some(len))) => {
            headers.typed_insert::<headers::ContentLength>(headers::ContentLength(*len as u64));
            Some(Encoding::FixedLength(*len))
        }

        (false, false, None, BodyInner::Reader(_, None)) => {
            headers.insert("transfer-encoding", HeaderValue::from_static("chunked"));
            Some(Encoding::Chunked)
        }
    };

    stream.write_all(format!("{:?} {}\r\n", parts.version, parts.status).as_bytes())?;

    for (name, val) in headers.iter() {
        stream.write_all(format!("{name}: ").as_bytes())?;
        stream.write_all(val.as_bytes())?;
        stream.write_all(b"\r\n")?;
    }

    stream.write_all(b"\r\n")?;
    stream.flush()?;

    match body.0 {
        BodyInner::Empty => {}

        BodyInner::Buffered(buf) => match encoding {
            Some(Encoding::CloseDelimited) => {
                stream.write_all(&buf)?;
            }
            Some(Encoding::FixedLength(len)) => {
                stream.write_all(&buf[0..len as usize])?;
            }
            Some(Encoding::Chunked) => {
                // TODO: Should we automatically split the responses into chunks here?
                stream.write_all(format!("{:x}\r\n", buf.len()).as_bytes())?;
                stream.write_all(&buf)?;
                stream.write_all(b"\r\n")?;
                stream.write_all(b"0\r\n\r\n")?;
            }
            None => {}
        },

        BodyInner::Chunked(chunks) => {
            for chunk in chunks {
                stream.write_all(format!("{:x}\r\n", chunk.len()).as_bytes())?;
                stream.write_all(&chunk)?;
                stream.write_all(b"\r\n")?;
                stream.flush()?;
            }
            stream.write_all(b"0\r\n\r\n")?;
        }

        BodyInner::Reader(mut reader, _) => match encoding {
            Some(Encoding::CloseDelimited) => {
                io::copy(&mut reader, stream)?;
            }
            Some(Encoding::FixedLength(len)) => {
                io::copy(&mut reader.take(len as u64), stream)?;
            }
            Some(Encoding::Chunked) => {
                let mut buf = [0_u8; 1024 * 8];
                loop {
                    let read = reader.read(&mut buf)?;
                    if read == 0 {
                        break;
                    }
                    stream.write_all(format!("{:x}\r\n", read).as_bytes())?;
                    stream.write_all(&buf[0..read])?;
                    stream.write_all(b"\r\n")?;
                    stream.flush()?;
                }
                stream.write_all(b"0\r\n\r\n")?;
            }
            None => {}
        },
    };

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use http::{Response, StatusCode};

    #[test]
    fn writes_responses_without_bodies() {
        let res = Response::builder()
            .status(StatusCode::OK)
            .header("some", "header")
            .body(Body::empty())
            .unwrap();

        let mut output: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        write_response(res, &mut output).unwrap();

        assert_eq!(output.get_ref(), b"HTTP/1.1 200 OK\r\nsome: header\r\n\r\n");
    }

    #[test]
    fn writes_responses_with_bodies() {
        let res = Response::builder()
            .status(StatusCode::OK)
            .body("lol".into())
            .unwrap();

        let mut output: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        write_response(res, &mut output).unwrap();

        assert_eq!(
            output.get_ref(),
            b"HTTP/1.1 200 OK\r\ncontent-length: 3\r\n\r\nlol"
        );
    }

    #[test]
    fn fails_when_the_informed_content_length_does_not_match_the_body_length() {
        let res = Response::builder()
            .status(StatusCode::OK)
            .header("content-length", "5")
            .body("lol".into())
            .unwrap();

        let mut output: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        assert!(write_response(res, &mut output).is_err());
    }

    #[test]
    fn writes_chunked_responses() {
        let res = Response::builder()
            .status(StatusCode::OK)
            .body(Body::chunked(vec![b"chunk1".to_vec(), b"chunk2".to_vec()]))
            .unwrap();

        let mut output: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        write_response(res, &mut output).unwrap();

        assert_eq!(
            output.get_ref(),
            b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n6\r\nchunk1\r\n6\r\nchunk2\r\n0\r\n\r\n"
        );
    }

    #[test]
    fn writes_responses_from_reader() {
        let res = Response::builder()
            .status(StatusCode::OK)
            .body(Body::from_reader(Cursor::new(b"lol"), Some(3)))
            .unwrap();

        let mut output: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        write_response(res, &mut output).unwrap();

        assert_eq!(
            output.get_ref(),
            b"HTTP/1.1 200 OK\r\ncontent-length: 3\r\n\r\nlol"
        );
    }

    #[test]
    fn limits_the_from_reader_response_body_size() {
        let res = Response::builder()
            .status(StatusCode::OK)
            .body(Body::from_reader(Cursor::new(b"lolwut"), Some(3)))
            .unwrap();

        let mut output: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        write_response(res, &mut output).unwrap();

        assert_eq!(
            output.get_ref(),
            b"HTTP/1.1 200 OK\r\ncontent-length: 3\r\n\r\nlol"
        );
    }

    #[test]
    fn uses_chunked_transfer_when_the_reader_size_is_undefined() {
        let res = Response::builder()
            .status(StatusCode::OK)
            .body(Body::from_reader(Cursor::new(b"lolwut"), None))
            .unwrap();

        let mut output: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        write_response(res, &mut output).unwrap();

        assert_eq!(
            std::str::from_utf8(output.get_ref()).unwrap(),
            "HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n6\r\nlolwut\r\n0\r\n\r\n"
        );
    }
}
