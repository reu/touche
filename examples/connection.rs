use std::{
    convert::Infallible,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use touche::{Response, Server, StatusCode};

fn main() -> std::io::Result<()> {
    let conns = Arc::new(AtomicUsize::new(0));

    Server::builder()
        .bind("0.0.0.0:4444")
        // The explicit &_ is necessary due a regression
        // See: https://github.com/rust-lang/rust/issues/81511
        .serve_connection(move |_conn: &_| {
            let conns = conns.clone();

            conns.fetch_add(1, Ordering::Relaxed);

            Ok::<_, Infallible>(move |_req| {
                let count = conns.load(Ordering::Relaxed);

                Response::builder()
                    .status(StatusCode::OK)
                    .body(format!("Connections: {count}"))
            })
        })
}
