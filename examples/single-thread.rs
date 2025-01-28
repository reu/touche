use touche::{Response, Server, StatusCode};

fn main() -> std::io::Result<()> {
    let mut counter = 0;
    Server::bind("0.0.0.0:4444").serve_single_thread(|_| {
        counter += 1;
        Response::builder()
            .status(StatusCode::OK)
            .body(format!("Request count: {}", counter))
    })
}
