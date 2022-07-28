use http::{Method, Request, Response, StatusCode};
use touche::{body::HttpBody, Body, Server};

fn main() -> std::io::Result<()> {
    Server::builder()
        .bind("0.0.0.0:4444")
        .serve(|req: Request<Body>| {
            match (req.method(), req.uri().path()) {
                (_, "/") => Response::builder()
                    .status(StatusCode::OK)
                    .body(Body::from("Usage: curl -d hello localhost:4444/echo\n")),

                // Responds with the same payload
                (&Method::POST, "/echo") => Response::builder()
                    .status(StatusCode::OK)
                    .body(req.into_body()),

                // Responds with the reversed payload
                (&Method::POST, "/reverse") => {
                    let body = req.into_body().into_bytes().unwrap_or_default();

                    match std::str::from_utf8(&body) {
                        Ok(message) => Response::builder()
                            .status(StatusCode::OK)
                            .body(message.chars().rev().collect::<String>().into()),

                        Err(err) => Response::builder()
                            .status(StatusCode::BAD_REQUEST)
                            .body(err.to_string().into()),
                    }
                }

                _ => Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::empty()),
            }
        })
}
