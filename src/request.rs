use std::io::{self, BufRead, Read};

use headers::HeaderMapExt;
use http::{Method, Request, Version};
use thiserror::Error;

use crate::body::Body;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("connection closed")]
    ConnectionClosed,
    #[error("io error")]
    Io(#[from] io::Error),
    #[error("invalid request")]
    Invalid(#[from] httparse::Error),
    #[error("incomplete request")]
    IncompleteRequest,
    #[error("unsupported http version: {0}")]
    UnsupportedHttpVersion(u8),
    #[error("invalid Transfer-Encoding header")]
    InvalidTransferEncoding,
    #[error("invalid header")]
    InvalidHeader(#[from] headers::Error),
    #[error("invalid chunk size")]
    InvalidChunkSize,
    #[error("failed to parse http request")]
    Unknown,
}

pub(crate) fn parse_request(
    mut stream: impl BufRead + 'static,
) -> Result<Request<Body>, ParseError> {
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
        return Err(ParseError::ConnectionClosed);
    }

    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = httparse::Request::new(&mut headers);
    req.parse(&buf)?;

    let method = req
        .method
        .map(|method| method.as_bytes())
        .ok_or(ParseError::IncompleteRequest)?;

    let path = req.path.ok_or(ParseError::IncompleteRequest)?;

    let version = match req.version.ok_or(ParseError::IncompleteRequest)? {
        0 => Version::HTTP_10,
        1 => Version::HTTP_11,
        version => return Err(ParseError::UnsupportedHttpVersion(version)),
    };

    let request = Request::builder()
        .method(Method::from_bytes(method).map_err(|_| ParseError::IncompleteRequest)?)
        .uri(path)
        .version(version);

    let request = headers
        .into_iter()
        .take_while(|header| *header != httparse::EMPTY_HEADER)
        .map(|header| (header.name, header.value))
        .fold(request, |req, (name, value)| req.header(name, value));

    let headers = request.headers_ref().ok_or(ParseError::Unknown)?;

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
    } else {
        Body::empty()
    };

    request.body(body).map_err(|_| ParseError::Unknown)
}

struct ChunkedReader(Box<dyn BufRead>);

impl Iterator for ChunkedReader {
    type Item = Vec<u8>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut buf = Vec::new();

        loop {
            if self.0.read_until(b'\n', &mut buf).ok()? == 0 {
                return None;
            }

            match httparse::parse_chunk_size(&buf) {
                Ok(httparse::Status::Complete((_pos, size))) if size == 0 => {
                    return None;
                }
                Ok(httparse::Status::Complete((_pos, size))) => {
                    let mut chunk = vec![0_u8; size as usize];
                    self.0.read_exact(&mut chunk).ok()?;
                    self.0.read_until(b'\n', &mut buf).ok()?;
                    return Some(chunk);
                }
                Ok(httparse::Status::Partial) => continue,
                Err(_) => return None,
            }
        }
    }
}

#[cfg(test)]
mod test {
    use crate::body::HttpBody;

    use super::*;

    #[test]
    fn parse_request_without_body() {
        let req = "GET /lolwut HTTP/1.1\r\nHost: lol.com\r\n\r\n";
        let req = std::io::Cursor::new(req);

        let req = parse_request(req).unwrap();

        assert_eq!(Version::HTTP_11, req.version());
        assert_eq!("/lolwut", req.uri().path());
        assert_eq!(
            Some("lol.com"),
            req.headers()
                .get(http::header::HOST)
                .and_then(|v| v.to_str().ok())
        );
    }

    #[test]
    fn parse_request_with_content_length_body() {
        let req = "POST /lol HTTP/1.1\r\nHost: lol.com\r\nContent-Length: 6\r\n\r\nlolwut ignored";
        let req = std::io::Cursor::new(req);

        let req = parse_request(req).unwrap();

        assert_eq!(req.into_body().into_bytes().unwrap(), b"lolwut");
    }

    #[test]
    fn parse_request_with_chunked_body() {
        let req = "POST /lol HTTP/1.1\r\nHost: lol.com\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nlol\r\n3\r\nwut\r\n0\r\n\r\n";
        let req = std::io::Cursor::new(req);

        let req = parse_request(req).unwrap();

        assert_eq!(req.into_body().into_bytes().unwrap(), b"lolwut");
    }

    #[test]
    fn parse_request_with_chunked_body_and_extensions() {
        let req = "POST /lol HTTP/1.1\r\nHost: lol.com\r\nTransfer-Encoding: chunked\r\n\r\n3;extension\r\nlol\r\n3\r\nwut\r\n0\r\n\r\n";
        let req = std::io::Cursor::new(req);

        let req = parse_request(req).unwrap();

        assert_eq!(req.into_body().into_bytes().unwrap(), b"lolwut");
    }

    #[test]
    fn parse_request_with_streaming_body() {
        let req = b"POST /lol HTTP/1.1\r\nHost: lol.com\r\nContent-Length: 2048\r\n\r\n";
        let body = [65_u8; 2048];
        let req = std::io::Cursor::new([req.as_ref(), body.as_ref()].concat());

        let req = parse_request(req).unwrap();

        assert_eq!(req.into_body().into_bytes().unwrap(), body);
    }

    #[test]
    fn fails_to_parse_incomplete_request() {
        let req = std::io::Cursor::new("POST /lol");

        assert!(matches!(
            parse_request(req),
            Err(ParseError::IncompleteRequest)
        ));
    }
}
