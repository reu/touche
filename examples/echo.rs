use std::net::TcpListener;

use http::{Response, StatusCode};

fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:4444")?;

    let (mut stream, _addr) = listener.accept()?;

    shrike::serve(&mut stream, |req| {
        Response::builder()
            .status(StatusCode::OK)
            .body(req.into_body().as_bytes().into())
    })
}
