use std::io::Read;

use touche::{multipart::multipart_request, Body, Request, Response, Server, StatusCode};

// Test with `curl --form file='@examples/multipart.rs' --form file='@Cargo.toml' localhost:4444`
fn main() -> std::io::Result<()> {
    Server::bind("0.0.0.0:4444").serve(|req: Request<Body>| match multipart_request(req) {
        Ok(mut multipart) => {
            while let Ok(Some(mut part)) = multipart.read_entry() {
                let mut buf = Vec::new();
                match part.data.read_to_end(&mut buf) {
                    Ok(len) => println!("Read part with length {len}"),
                    Err(err) => println!("Failed to read part: {err}"),
                }
            }
            Response::builder().status(StatusCode::OK).body("Ok")
        }
        Err(_) => Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body("Invalid multipart request"),
    })
}
