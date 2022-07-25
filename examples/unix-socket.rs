use std::{os::unix::net::UnixListener, thread, fs};

use http::{Response, StatusCode};

// Run with: curl --unix-socket examples/unix-socket.socket http://localhost
fn main() -> std::io::Result<()> {
    fs::remove_file("./examples/unix-socket.socket")?;
    let listener = UnixListener::bind("./examples/unix-socket.socket")?;

    for stream in listener.incoming() {
        let stream = stream?;
        thread::spawn(move || {
            shrike::serve(stream, |_req| {
                Response::builder()
                    .status(StatusCode::OK)
                    .body("Hello, world!")
            })
            .ok();
        });
    }

    Ok(())
}
