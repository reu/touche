use std::{error::Error, thread};

use http::{Response, StatusCode};
use touche::{Body, Server};

fn main() -> std::io::Result<()> {
    Server::bind("0.0.0.0:4444").serve(|_req| {
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
    })
}
