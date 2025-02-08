use std::path::PathBuf;

use touche::{Body, Response, Server, StatusCode};

fn main() -> std::io::Result<()> {
    Server::bind("0.0.0.0:4444").serve(|_req| {
        match Body::try_from(PathBuf::from("./examples/file.rs")) {
            Ok(file) => Response::builder().status(StatusCode::OK).body(file),
            Err(_) => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::empty()),
        }
    })
}
