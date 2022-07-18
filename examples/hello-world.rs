use std::{env, net::TcpListener};

use http::{Response, StatusCode};
use shrike::Connection;
use threadpool::ThreadPool;

fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:4444")?;

    let threads = match env::var("THREADS") {
        Ok(threads) => threads.parse::<usize>().expect("Invalid THREADS value"),
        Err(_) => 100,
    };

    let pool = ThreadPool::new(threads);

    for stream in listener.incoming() {
        let stream = stream?;
        pool.execute(move || {
            let mut stream = stream;
            while let Ok(Connection::KeepAlive(conn)) = shrike::serve(stream, |_req| {
                Response::builder()
                    .status(StatusCode::OK)
                    .body("Hello, world!".into())
            }) {
                stream = conn;
            }
        });
    }

    Ok(())
}
