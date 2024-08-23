// Run with: curl --unix-socket examples/unix-socket.socket http://localhost
#[cfg(feature = "unix-sockets")]
fn main() -> std::io::Result<()> {
    use std::{fs, os::unix::net::UnixListener};
    use touche::{Response, Server, StatusCode};

    fs::remove_file("./examples/unix-socket.socket").ok();

    let listener = UnixListener::bind("./examples/unix-socket.socket")?;

    let connections = listener
        .incoming()
        .filter_map(|conn| conn.ok())
        .map(|conn| conn.into());

    Server::builder()
        .max_threads(100)
        .from_connections(connections)
        .serve(|_req| {
            Response::builder()
                .status(StatusCode::OK)
                .body("Hello from Unix socket!")
        })
}

#[cfg(not(feature = "unix-sockets"))]
fn main() {
    println!("This example requires the unix-sockets feature to be enabled");
}
