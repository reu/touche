use std::net::TcpListener;

use http::{Response, StatusCode};
use shrike::Request;

fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:4444")?;

    for stream in listener.incoming() {
        shrike::serve(stream?, |req: Request| {
            Response::builder()
                .status(StatusCode::OK)
                .body(req.into_body())
        })?;
    }

    Ok(())
}
