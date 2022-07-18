use std::{error::Error, net::TcpListener, sync::mpsc, thread, time::Duration};

use http::{Response, StatusCode};
use shrike::Body;

fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:4444")?;

    for stream in listener.incoming() {
        shrike::serve(stream?, |_req| {
            let (tx, rx) = mpsc::channel();

            thread::spawn(move || {
                tx.send("chunk1")?;
                thread::sleep(Duration::from_secs(1));
                tx.send("chunk2")?;
                thread::sleep(Duration::from_secs(1));
                tx.send("chunk3")?;
                Ok::<_, Box<dyn Error + Send + Sync>>(())
            });

            Response::builder()
                .status(StatusCode::OK)
                // Disable buffering on Chrome
                .header("X-Content-Type-Options", "nosniff")
                .body(Body::chunked(rx))
        })?;
    }

    Ok(())
}
