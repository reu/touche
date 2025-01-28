use std::convert::Infallible;

use headers::HeaderMapExt;
use touche::{server::Service, Body, Request, Response, Server, StatusCode};

#[derive(Clone)]
struct UploadService {
    max_length: u64,
}

impl Service for UploadService {
    type Body = &'static str;
    type Error = Infallible;

    fn call(&mut self, _req: Request<Body>) -> Result<http::Response<Self::Body>, Self::Error> {
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
    Server::bind("0.0.0.0:4444").serve(UploadService { max_length: 1024 })
}
