use std::env;

use http::{Response, StatusCode};
use touche::Server;

fn main() -> std::io::Result<()> {
    let threads = match env::var("THREADS") {
        Ok(threads) => threads.parse::<usize>().expect("Invalid THREADS value"),
        Err(_) => 100,
    };

    Server::builder()
        .max_threads(threads)
        .bind("0.0.0.0:4444")
        .serve(|_req| {
            Response::builder()
                .status(StatusCode::OK)
                .body("Hello, world!")
        })
}
