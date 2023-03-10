use std::io::{self, BufRead, Write};

use headers::{HeaderMap, HeaderMapExt};
use http::{response::Parts, StatusCode, Version};

use crate::{
    body::Chunk,
    request::{ChunkedReader, ParseError},
    upgrade::UpgradeExtension,
    Body, HttpBody,
};

#[derive(PartialEq, Eq)]
pub(crate) enum Encoding {
    FixedLength(u64),
    Chunked,
    CloseDelimited,
}

pub(crate) enum Outcome {
    Close,
    KeepAlive,
    Upgrade(UpgradeExtension),
}

pub(crate) fn parse_response(
    mut stream: impl BufRead + Send + 'static,
) -> Result<http::Response<Body>, ParseError> {
    let mut buf = Vec::with_capacity(800);

    loop {
        if stream.read_until(b'\n', &mut buf)? == 0 {
            break;
        }

        match buf.as_slice() {
            [.., b'\r', b'\n', b'\r', b'\n'] => break,
            [.., b'\n', b'\n'] => break,
            _ => continue,
        }
    }

    if buf.is_empty() {
        return Err(ParseError::IncompleteRequest);
    }

    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut res = httparse::Response::new(&mut headers);
    res.parse(&buf)?;

    let status = res
        .code
        .and_then(|code| StatusCode::from_u16(code).ok())
        .ok_or(ParseError::IncompleteRequest)?;

    let version = match res.version.ok_or(ParseError::IncompleteRequest)? {
        0 => Version::HTTP_10,
        1 => Version::HTTP_11,
        version => return Err(ParseError::UnsupportedHttpVersion(version)),
    };

    let res = http::Response::builder().version(version).status(status);

    let res = headers
        .into_iter()
        .take_while(|header| *header != httparse::EMPTY_HEADER)
        .map(|header| (header.name, header.value))
        .fold(res, |res, (name, value)| res.header(name, value));

    let headers = res.headers_ref().ok_or(ParseError::Unknown)?;

    let body = if let Some(encoding) = headers.typed_try_get::<headers::TransferEncoding>()? {
        if !encoding.is_chunked() {
            // https://datatracker.ietf.org/doc/html/rfc2616#section-3.6
            return Err(ParseError::InvalidTransferEncoding);
        }
        Body::from_iter(ChunkedReader(Box::new(stream)))
    } else if let Some(len) = headers.typed_try_get::<headers::ContentLength>()? {
        // Let's automatically buffer small bodies
        if len.0 < 1024 {
            let mut buf = vec![0_u8; len.0 as usize];
            stream.read_exact(&mut buf)?;
            Body::from(buf)
        } else {
            Body::from_reader(stream, len.0 as usize)
        }
    } else if headers
        .typed_get::<headers::Connection>()
        .filter(|conn| conn.contains("close"))
        .is_some()
    {
        Body::from_reader(stream, None)
    } else {
        Body::empty()
    };

    res.body(body).map_err(|_| ParseError::Unknown)
}

pub(crate) fn write_response<B: HttpBody>(
    res: http::Response<B>,
    stream: &mut impl Write,
    write_body: bool,
) -> io::Result<Outcome> {
    let (
        Parts {
            status,
            version,
            mut headers,
            mut extensions,
            ..
        },
        body,
    ) = res.into_parts();

    let has_chunked_encoding = headers
        .typed_get::<headers::TransferEncoding>()
        .filter(|te| te.is_chunked())
        .is_some();

    let has_connection_close = headers
        .typed_get::<headers::Connection>()
        .filter(|conn| conn.contains("close"))
        .is_some();

    let content_length = headers.typed_get::<headers::ContentLength>();

    let encoding = if has_chunked_encoding && version == Version::HTTP_11 {
        Encoding::Chunked
    } else if content_length.is_some() || body.len().is_some() {
        match (content_length, body.len()) {
            (Some(len), Some(body_len)) => {
                if len.0 != body_len {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        "content-length doesn't match body length",
                    ));
                }
                Encoding::FixedLength(len.0)
            }
            (Some(len), None) => Encoding::FixedLength(len.0),
            (None, Some(len)) => {
                headers.typed_insert::<headers::ContentLength>(headers::ContentLength(len));
                Encoding::FixedLength(len)
            }
            (None, None) => unreachable!(),
        }
    } else if body.len().is_none() && !has_connection_close && version == Version::HTTP_11 {
        headers.typed_insert::<headers::TransferEncoding>(headers::TransferEncoding::chunked());
        Encoding::Chunked
    } else {
        if !has_connection_close {
            headers.typed_insert::<headers::Connection>(headers::Connection::close());
        }
        Encoding::CloseDelimited
    };

    if version == Version::HTTP_10 && has_chunked_encoding {
        headers.remove(http::header::TRANSFER_ENCODING);
    };

    stream.write_all(format!("{version:?} {status}\r\n").as_bytes())?;

    for (name, val) in headers.iter() {
        stream.write_all(&[format!("{name}: ").as_bytes(), val.as_bytes(), b"\r\n"].concat())?;
    }

    stream.write_all(b"\r\n")?;

    if write_body {
        match encoding {
            // Just buffer small bodies
            Encoding::FixedLength(len) if len < 1024 => {
                stream.write_all(&body.into_bytes()?)?;
            }
            Encoding::FixedLength(_) | Encoding::CloseDelimited => {
                io::copy(&mut body.into_reader(), stream)?;
            }
            Encoding::Chunked => {
                let mut trailers = HeaderMap::new();

                for chunk in body.into_chunks() {
                    match chunk? {
                        Chunk::Data(chunk) => {
                            stream.write_all(format!("{:x}\r\n", chunk.len()).as_bytes())?;
                            stream.write_all(&chunk)?;
                            stream.write_all(b"\r\n")?;
                            stream.flush()?;
                        }
                        Chunk::Trailers(te) => {
                            trailers.extend(te);
                        }
                    }
                }

                stream.write_all(b"0\r\n")?;
                for (name, val) in trailers.iter() {
                    stream.write_all(
                        &[format!("{name}: ").as_bytes(), val.as_bytes(), b"\r\n"].concat(),
                    )?;
                }
                stream.write_all(b"\r\n")?;
            }
        };
    }

    let connection = headers.typed_get::<headers::Connection>();

    let outcome = if let Some(upgrade) = extensions.remove::<UpgradeExtension>() {
        Outcome::Upgrade(upgrade)
    } else if encoding == Encoding::CloseDelimited
        || connection.filter(|conn| conn.contains("close")).is_some()
    {
        Outcome::Close
    } else {
        Outcome::KeepAlive
    };

    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, thread};

    use crate::{upgrade::Upgrade, Body};

    use super::*;
    use http::{Response, StatusCode};

    #[test]
    fn writes_responses_without_bodies() {
        let res = Response::builder()
            .status(StatusCode::OK)
            .body(Body::empty())
            .unwrap();

        let mut output: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        let outcome = write_response(res, &mut output, true).unwrap();

        assert_eq!(
            output.get_ref(),
            b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n"
        );
        assert!(matches!(outcome, Outcome::KeepAlive));
    }

    #[test]
    fn writes_responses_with_bodies() {
        let res = Response::builder()
            .status(StatusCode::OK)
            .body("lol")
            .unwrap();

        let mut output: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        let outcome = write_response(res, &mut output, true).unwrap();

        assert_eq!(
            output.get_ref(),
            b"HTTP/1.1 200 OK\r\ncontent-length: 3\r\n\r\nlol"
        );
        assert!(matches!(outcome, Outcome::KeepAlive));
    }

    #[test]
    fn allows_to_skip_body_writing() {
        let res = Response::builder()
            .status(StatusCode::OK)
            .body("lol")
            .unwrap();

        let mut output: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        let outcome = write_response(res, &mut output, false).unwrap();

        assert_eq!(
            output.get_ref(),
            b"HTTP/1.1 200 OK\r\ncontent-length: 3\r\n\r\n"
        );
        assert!(matches!(outcome, Outcome::KeepAlive));
    }

    #[test]
    fn fails_when_the_informed_content_length_does_not_match_the_body_length() {
        let res = Response::builder()
            .status(StatusCode::OK)
            .header("content-length", "5")
            .body("lol")
            .unwrap();

        let mut output: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        assert!(write_response(res, &mut output, true).is_err());
    }

    #[test]
    fn writes_chunked_responses() {
        let res = Response::builder()
            .status(StatusCode::OK)
            .header("transfer-encoding", "chunked")
            .body(Body::from_iter(vec![
                b"chunk1".to_vec(),
                b"chunk2".to_vec(),
            ]))
            .unwrap();

        let mut output: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        let outcome = write_response(res, &mut output, true).unwrap();

        assert_eq!(
            output.get_ref(),
            b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n6\r\nchunk1\r\n6\r\nchunk2\r\n0\r\n\r\n"
        );
        assert!(matches!(outcome, Outcome::KeepAlive));
    }

    #[test]
    fn writes_chunked_responses_with_trailers() {
        let (sender, body) = Body::channel();

        let send_thread = thread::spawn(move || {
            sender.send("lol").unwrap();
            sender.send("wut").unwrap();
            sender.send_trailer("content-length", "6").unwrap();
        });

        let res = Response::builder()
            .status(StatusCode::OK)
            .header("trailers", "content-length")
            .body(body)
            .unwrap();

        let mut output: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        let outcome = write_response(res, &mut output, true).unwrap();

        send_thread.join().unwrap();

        assert_eq!(
            output.get_ref(),
            b"HTTP/1.1 200 OK\r\ntrailers: content-length\r\ntransfer-encoding: chunked\r\n\r\n3\r\nlol\r\n3\r\nwut\r\n0\r\ncontent-length: 6\r\n\r\n"
        );
        assert!(matches!(outcome, Outcome::KeepAlive));
    }

    #[test]
    fn writes_responses_from_reader_with_known_size() {
        let res = Response::builder()
            .status(StatusCode::OK)
            .body(Body::from_reader(Cursor::new(b"lol"), Some(3)))
            .unwrap();

        let mut output: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        let outcome = write_response(res, &mut output, true).unwrap();

        assert_eq!(
            output.get_ref(),
            b"HTTP/1.1 200 OK\r\ncontent-length: 3\r\n\r\nlol"
        );
        assert!(matches!(outcome, Outcome::KeepAlive));
    }

    #[test]
    fn limits_the_from_reader_response_body_size() {
        let res = Response::builder()
            .status(StatusCode::OK)
            .body(Body::from_reader(Cursor::new(b"lolwut"), Some(3)))
            .unwrap();

        let mut output: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        let outcome = write_response(res, &mut output, true).unwrap();

        assert_eq!(
            output.get_ref(),
            b"HTTP/1.1 200 OK\r\ncontent-length: 3\r\n\r\nlol"
        );
        assert!(matches!(outcome, Outcome::KeepAlive));
    }

    #[test]
    fn uses_chunked_transfer_when_the_reader_size_is_undefined() {
        let res = Response::builder()
            .status(StatusCode::OK)
            .body(Body::from_reader(Cursor::new(b"lolwut"), None))
            .unwrap();

        let mut output: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        let outcome = write_response(res, &mut output, true).unwrap();

        assert_eq!(
            output.get_ref(),
            b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n6\r\nlolwut\r\n0\r\n\r\n"
        );
        assert!(matches!(outcome, Outcome::KeepAlive));
    }

    #[test]
    fn does_not_use_chunked_encoding_when_the_reader_size_is_undefined_and_connection_is_close() {
        let res = Response::builder()
            .status(StatusCode::OK)
            .header("connection", "close")
            .body(Body::from_reader(Cursor::new(b"lolwut"), None))
            .unwrap();

        let mut output: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        let outcome = write_response(res, &mut output, true).unwrap();

        assert_eq!(
            output.get_ref(),
            b"HTTP/1.1 200 OK\r\nconnection: close\r\n\r\nlolwut"
        );
        assert!(matches!(outcome, Outcome::Close));
    }

    #[test]
    fn supports_channel_response_bodies() {
        let (sender, body) = Body::channel();

        let send_thread = thread::spawn(move || {
            sender.send("lol").unwrap();
            sender.send("wut").unwrap();
        });

        let res = Response::builder()
            .status(StatusCode::OK)
            .header("connection", "close")
            .body(body)
            .unwrap();

        let mut output: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        let outcome = write_response(res, &mut output, true).unwrap();

        send_thread.join().unwrap();

        assert_eq!(
            std::str::from_utf8(output.get_ref()).unwrap(),
            "HTTP/1.1 200 OK\r\nconnection: close\r\n\r\nlolwut"
        );
        assert!(matches!(outcome, Outcome::Close));
    }

    #[test]
    fn returns_a_close_connection_outcome_when_informed_an_explicit_close_connection_header() {
        let res = Response::builder()
            .status(StatusCode::OK)
            .header("connection", "close")
            .body(Body::empty())
            .unwrap();

        let mut output: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        let outcome = write_response(res, &mut output, true).unwrap();

        assert!(matches!(outcome, Outcome::Close));
    }

    #[test]
    fn returns_a_close_keep_alive_outcome_when_no_close_connection_is_informed() {
        let res = Response::builder()
            .status(StatusCode::OK)
            .body(Body::empty())
            .unwrap();

        let mut output: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        let outcome = write_response(res, &mut output, true).unwrap();

        assert!(matches!(outcome, Outcome::KeepAlive));
    }

    #[test]
    fn returns_upgrade_outcome() {
        let res = Response::builder()
            .status(StatusCode::SWITCHING_PROTOCOLS)
            .upgrade(|_| {})
            .body(Body::empty())
            .unwrap();

        let mut output: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        let outcome = write_response(res, &mut output, true).unwrap();

        assert!(matches!(outcome, Outcome::Upgrade(_)));
    }

    #[test]
    fn writes_http_10_responses() {
        let res = Response::builder()
            .status(StatusCode::OK)
            .version(Version::HTTP_10)
            .body("lol")
            .unwrap();

        let mut output: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        let outcome = write_response(res, &mut output, true).unwrap();

        assert_eq!(
            output.get_ref(),
            b"HTTP/1.0 200 OK\r\ncontent-length: 3\r\n\r\nlol"
        );
        assert!(matches!(outcome, Outcome::KeepAlive));
    }

    #[test]
    fn removes_chunked_transfer_encoding_from_http_10_responses() {
        let res = Response::builder()
            .status(StatusCode::OK)
            .version(Version::HTTP_10)
            .header("transfer-encoding", "chunked")
            .body(Body::from_iter(std::iter::once("lol")))
            .unwrap();

        let mut output: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        let outcome = write_response(res, &mut output, true).unwrap();

        assert_eq!(
            output.get_ref(),
            b"HTTP/1.0 200 OK\r\nconnection: close\r\n\r\nlol"
        );
        assert!(matches!(outcome, Outcome::Close));
    }

    #[test]
    fn parse_response_without_body() {
        let res = "HTTP/1.1 200 OK\r\ndate: Mon, 25 Jul 2022 21:34:35 GMT\r\n\r\n";
        let res = Cursor::new(res);

        let res = parse_response(res).unwrap();

        assert_eq!(Version::HTTP_11, res.version());
        assert_eq!(StatusCode::OK, res.status());
        assert_eq!(
            Some("Mon, 25 Jul 2022 21:34:35 GMT"),
            res.headers()
                .get(http::header::DATE)
                .and_then(|v| v.to_str().ok())
        );
    }

    #[test]
    fn parse_response_with_content_length_body() {
        let res = "HTTP/1.1 200 OK\r\ncontent-length: 6\r\n\r\nlolwut ignored";
        let res = Cursor::new(res);

        let res = parse_response(res).unwrap();

        assert_eq!(res.into_body().into_bytes().unwrap(), b"lolwut");
    }

    #[test]
    fn parse_response_with_chunked_body() {
        let res = "HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n3\r\nlol\r\n3\r\nwut\r\n0\r\n\r\n";
        let res = Cursor::new(res);

        let res = parse_response(res).unwrap();

        assert_eq!(res.into_body().into_bytes().unwrap(), b"lolwut");
    }

    #[test]
    fn parse_response_with_chunked_body_and_extensions() {
        let res = "HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n3;extension\r\nlol\r\n3\r\nwut\r\n0\r\n\r\n";
        let res = Cursor::new(res);

        let res = parse_response(res).unwrap();

        assert_eq!(res.into_body().into_bytes().unwrap(), b"lolwut");
    }

    #[test]
    fn parse_response_with_streaming_body() {
        let res = b"HTTP/1.1 200 OK\r\ncontent-length: 2048\r\n\r\n";
        let body = [65_u8; 2048];
        let res = Cursor::new([res.as_ref(), body.as_ref()].concat());

        let res = parse_response(res).unwrap();

        assert_eq!(res.into_body().into_bytes().unwrap(), body);
    }

    #[test]
    fn parse_response_with_close_delimited_body() {
        let res = "HTTP/1.1 200 OK\r\nconnection: close\r\n\r\nlolwut";
        let res = Cursor::new(res);

        let res = parse_response(res).unwrap();

        assert_eq!(res.into_body().into_bytes().unwrap(), b"lolwut");
    }
}
