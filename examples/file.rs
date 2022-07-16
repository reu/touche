use std::{error::Error, fs, net::TcpListener};

use http::{Response, StatusCode};

fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:4444")?;

    for stream in listener.incoming() {
        shrike::serve(&mut stream?, |_req| {
            let file = fs::File::open("./examples/file.rs")?;
            Ok::<_, Box<dyn Error + Send + Sync>>(
                Response::builder()
                    .status(StatusCode::OK)
                    .body(file.try_into()?)?,
            )
        })?;
    }

    Ok(())
}
