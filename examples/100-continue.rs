use std::{convert::Infallible, net::TcpListener};

use headers::HeaderMapExt;
use http::{Response, StatusCode};
use shrike::{Body, Handler, Request};

struct UploadHandler {
    max_length: u64,
}

impl Handler<Body, Infallible> for UploadHandler {
    fn handle(&self, _req: Request) -> Result<http::Response<Body>, Infallible> {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(Body::from("Thanks for the info!"))
            .unwrap())
    }

    fn should_continue(&self, req: &Request) -> StatusCode {
        match req.headers().typed_get::<headers::ContentLength>() {
            Some(len) if len.0 <= self.max_length => StatusCode::CONTINUE,
            _ => StatusCode::EXPECTATION_FAILED,
        }
    }
}

fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:4444")?;

    for stream in listener.incoming() {
        // Refuses payloads that exceeds 1kb
        shrike::serve(stream?, UploadHandler { max_length: 1024 })?;
    }

    Ok(())
}
