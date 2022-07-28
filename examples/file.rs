use std::{fs, io};

use touche::{Body, Response, Server, StatusCode};

fn main() -> std::io::Result<()> {
    Server::bind("0.0.0.0:4444").serve(|_req| {
        let file = fs::File::open("./examples/file.rs")?;
        Ok::<_, io::Error>(
            Response::builder()
                .status(StatusCode::OK)
                .body(Body::try_from(file)?)
                .unwrap(),
        )
    })
}
