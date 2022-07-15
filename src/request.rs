use std::io::{self, BufRead};

use headers::HeaderMapExt;
use http::{Method, Request, Version};
use thiserror::Error;

// TODO: we should not automatically buffer the request body
#[derive(Debug)]
pub struct Body(Vec<u8>);

impl Body {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("data store disconnected")]
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
        return Err(ParseError::IncompleteRequest);
    }

    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = httparse::Request::new(&mut headers);
    req.parse(&buf)?;

    let method = req.method.map(|method| method.as_bytes()).unwrap();

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

        let mut buf = Vec::new();
        let mut body = Vec::new();

        loop {
            if stream.read_until(b'\n', &mut buf)? == 0 {
                break;
            }

            match httparse::parse_chunk_size(&buf) {
                Ok(httparse::Status::Complete((_pos, size))) if size == 0 => {
                    break;
                }
                Ok(httparse::Status::Complete((_pos, size))) => {
                    let mut chunk = vec![0_u8; size as usize];
                    stream.read_exact(&mut chunk)?;
                    stream.read_until(b'\n', &mut buf)?;
                    body.append(&mut chunk);
                    buf.clear();
                }
                Ok(httparse::Status::Partial) => continue,
                Err(_) => return Err(ParseError::InvalidChunkSize),
            }
        }
        Body(body)
    } else if let Some(len) = headers.typed_try_get::<headers::ContentLength>()? {
        let mut buf = vec![0_u8; len.0 as usize];
        stream.read_exact(&mut buf)?;
        Body(buf)
    } else if let Some(true) = headers
        .typed_try_get::<headers::Connection>()?
        .map(|conn| conn.contains("close"))
    {
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf)?;
        Body(buf)
    } else {
        Body(Vec::new())
    };

    request.body(body).map_err(|_| ParseError::Unknown)
}

#[cfg(test)]
mod test {
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
    fn parse_request_with_close_delimited_body() {
        let req = "POST /lol HTTP/1.1\r\nHost: lol.com\r\nConnection: close\r\n\r\nlolwut";
        let req = std::io::Cursor::new(req);

        let req = parse_request(req).unwrap();

        assert_eq!(*req.into_body().as_bytes(), b"lolwut"[..]);
    }

    #[test]
    fn parse_request_with_content_length_body() {
        let req = "POST /lol HTTP/1.1\r\nHost: lol.com\r\nContent-Length: 6\r\n\r\nlolwut ignored";
        let req = std::io::Cursor::new(req);

        let req = parse_request(req).unwrap();

        assert_eq!(*req.into_body().as_bytes(), b"lolwut"[..]);
    }

    #[test]
    fn parse_request_with_chunked_body() {
        let req = "POST /lol HTTP/1.1\r\nHost: lol.com\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nlol\r\n3\r\nwut\r\n0\r\n\r\n";
        let req = std::io::Cursor::new(req);

        let req = parse_request(req).unwrap();

        assert_eq!(*req.into_body().as_bytes(), b"lolwut"[..]);
    }

    #[test]
    fn parse_request_with_chunked_body_and_extensions() {
        let req = "POST /lol HTTP/1.1\r\nHost: lol.com\r\nTransfer-Encoding: chunked\r\n\r\n3;extension\r\nlol\r\n3\r\nwut\r\n0\r\n\r\n";
        let req = std::io::Cursor::new(req);

        let req = parse_request(req).unwrap();

        assert_eq!(*req.into_body().as_bytes(), b"lolwut"[..]);
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
