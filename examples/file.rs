use std::{error::Error, fs, net::TcpListener};

use http::{Response, StatusCode};
use shrike::Body;

fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:4444")?;

    for stream in listener.incoming() {
        shrike::serve(stream?, |_req| {
            let file = fs::File::open("./examples/file.rs")?;
            Ok::<_, Box<dyn Error + Send + Sync>>(
                Response::builder()
                    .status(StatusCode::OK)
                    .body(Body::try_from(file)?)?,
            )
        })?;
    }

    Ok(())
}
