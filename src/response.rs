use std::io::{self, Write};

#[derive(Default)]
pub struct Body(Vec<u8>);

impl Body {
    pub fn empty() -> Self {
        Body(Vec::new())
    }
}

impl From<Vec<u8>> for Body {
    fn from(body: Vec<u8>) -> Self {
        Self(body)
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

    if body.0.len() > 0 {
        stream.write_all(format!("content-length: {}\r\n", body.0.len()).as_bytes())?;
    }

    stream.write_all(b"\r\n")?;
    stream.write_all(&body.0)?;

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
}
