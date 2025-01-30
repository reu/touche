use std::convert::Infallible;

use touche::{Connection, Response, Server, StatusCode};

fn main() -> std::io::Result<()> {
    Server::builder()
        .bind("0.0.0.0:4444")
        // The explicit type is necessary due a regression
        // See: https://github.com/rust-lang/rust/issues/81511
        .make_service(move |_conn: &Connection| {
            // We are now allowed to have mutable state inside this connection
            let mut counter = 0;

            Ok::<_, Infallible>(move |_req| {
                counter += 1;

                Response::builder()
                    .status(StatusCode::OK)
                    .body(format!("Requests on this connection: {counter}"))
            })
        })
}
