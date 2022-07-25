use std::{error::Error, net::TcpListener, thread};

use http::{Response, StatusCode};
use touche::Body;

fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:4444")?;

    for stream in listener.incoming() {
        touche::serve(stream?, |_req| {
            let (channel, body) = Body::channel();

            thread::spawn(move || {
                let mut md5 = md5::Context::new();
                for chunk in ["chunk1", "chunk2", "chunk3"] {
                    channel.send(chunk)?;
                    md5.consume(chunk);
                }
                channel.send_trailer("content-md5", base64::encode(*md5.compute()))?;
                Ok::<_, Box<dyn Error + Send + Sync>>(())
            });

            Response::builder()
                .status(StatusCode::OK)
                .header("trailers", "content-md5")
                .body(body)
        })?;
    }

    Ok(())
}
