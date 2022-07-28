use std::{error::Error, thread, time::Duration};

use touche::{Body, Response, Server, StatusCode};

fn main() -> std::io::Result<()> {
    Server::bind("0.0.0.0:4444").serve(|_req| {
        let (channel, body) = Body::channel();

        thread::spawn(move || {
            channel.send("chunk1")?;
            thread::sleep(Duration::from_secs(1));
            channel.send("chunk2")?;
            thread::sleep(Duration::from_secs(1));
            channel.send("chunk3")?;
            Ok::<_, Box<dyn Error + Send + Sync>>(())
        });

        Response::builder()
            .status(StatusCode::OK)
            // Disable buffering on Chrome
            .header("X-Content-Type-Options", "nosniff")
            .body(body)
    })
}
