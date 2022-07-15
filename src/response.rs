use std::io::{self, Write};

use headers::HeaderMapExt;

#[derive(Default)]
pub enum Body {
    #[default]
    Empty,
    Buffered(Vec<u8>),
    Chunked(Box<dyn Iterator<Item = Vec<u8>>>),
}

impl Body {
    pub fn empty() -> Self {
        Body::Empty
    }

    pub fn chunked<T: Into<Vec<u8>>>(chunks: impl IntoIterator<Item = T> + 'static) -> Self {
        Body::Chunked(Box::new(chunks.into_iter().map(|chunk| chunk.into())))
    }
}

impl From<Vec<u8>> for Body {
    fn from(body: Vec<u8>) -> Self {
        Body::Buffered(body)
    }
}

impl From<&[u8]> for Body {
    fn from(body: &[u8]) -> Self {
        body.to_vec().into()
    }
}

impl From<&str> for Body {
    fn from(body: &str) -> Self {
        body.as_bytes().to_vec().into()
    }
}

impl From<String> for Body {
    fn from(body: String) -> Self {
        body.into_bytes().into()
    }
}

pub(crate) fn write_response(res: http::Response<Body>, stream: &mut impl Write) -> io::Result<()> {
    let (parts, body) = res.into_parts();

    stream.write_all(format!("{:?} {}\r\n", parts.version, parts.status).as_bytes())?;

    for (name, val) in parts.headers.iter() {
        stream.write_all(format!("{name}: ").as_bytes())?;
        stream.write_all(val.as_bytes())?;
        stream.write_all(b"\r\n")?;
    }

    match body {
        Body::Buffered(ref buf) if !buf.is_empty() => {
            match parts.headers.typed_get::<headers::ContentLength>() {
                Some(len) if len.0 != buf.len() as u64 => {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        "informed content-lenght header does not match body length",
                    ))
                }
                Some(_len) => {}
                None => {
                    stream.write_all(format!("content-length: {}\r\n", buf.len()).as_bytes())?
                }
            };
        }
        Body::Chunked(ref _chunks) => {
            stream.write_all(b"transfer-encoding: chunked\r\n")?;
        }
        _ => (),
    };

    stream.write_all(b"\r\n")?;

    match body {
        Body::Buffered(buf) => stream.write_all(&buf)?,
        Body::Chunked(chunks) => {
            stream.flush()?;
            for chunk in chunks {
                stream.write_all(format!("{:x}\r\n", chunk.len()).as_bytes())?;
                stream.write_all(&chunk)?;
                stream.write_all(b"\r\n")?;
                stream.flush()?;
            }
            stream.write_all(b"0\r\n\r\n")?;
        }
        _ => (),
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
}
