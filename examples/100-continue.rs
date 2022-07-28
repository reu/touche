use std::convert::Infallible;

use headers::HeaderMapExt;
use http::{Request, Response, StatusCode};
use touche::{App, Body, Server};

#[derive(Clone)]
struct UploadHandler {
    max_length: u64,
}

impl App for UploadHandler {
    type Body = &'static str;
    type Error = Infallible;

    fn handle(&self, _req: Request<Body>) -> Result<http::Response<&'static str>, Infallible> {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body("Thanks for the info!")
            .unwrap())
    }

    fn should_continue(&self, req: &Request<Body>) -> StatusCode {
        match req.headers().typed_get::<headers::ContentLength>() {
            Some(len) if len.0 <= self.max_length => StatusCode::CONTINUE,
            _ => StatusCode::EXPECTATION_FAILED,
        }
    }
}

fn main() -> std::io::Result<()> {
    Server::bind("0.0.0.0:4444").serve(UploadHandler { max_length: 1024 })
}
