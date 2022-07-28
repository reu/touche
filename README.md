# touche

Touché is a low level but fully featured HTTP 1.0/1.1 library.

It tries to mimic [hyper](https://crates.io/crates/hyper), but with a synchronous API.

More information can be found in the [crate documentation](https://docs.rs/touche).

## Hello world

```rust
use touche::{Response, Server, StatusCode};

fn main() -> std::io::Result<()> {
    Server::bind("0.0.0.0:4444").serve(|_req| {
        Response::builder()
            .status(StatusCode::OK)
            .body("Hello World")
    })
}
```

## Features
- HTTP Server (thread per connection design)
- ~HTTP Client~ (work in progress)
- Non buffered (streaming) requests and response bodies
- TLS support
- Upgrade connections
- Trailers headers
- 100 continue expectations support
- Unix sockets servers

## Comparison with Hyper

Touché follows some of the same designs as Hyper:

- Low level
- Uses the [http](https://crates.io/crates/http) crate to represent all the HTTP related types
- Allows fine granded implementations of an HttpBody
- Fully featured and correct

But also, there are some notable differences:
- It is synchronous
- It uses `Vec<u8>` to represent bytes instead of [Bytes](https://crates.io/crates/bytes)
- Doesn't support (and probably never will) HTTP 2

## Other examples

### Chunked response

```rust
use std::{error::Error, thread};

use touche::{Body, Response, Server, StatusCode};

fn main() -> std::io::Result<()> {
    Server::bind("0.0.0.0:4444").serve(|_req| {
        let (channel, body) = Body::channel();

        thread::spawn(move || {
            channel.send("chunk1").unwrap();
            channel.send("chunk2").unwrap();
            channel.send("chunk3").unwrap();
        });

        Response::builder()
            .status(StatusCode::OK)
            .body(body)
    })
}
```

### Naive routing with pattern matching

```rust
use touche::{body::HttpBody, Body, Method, Request, Response, Server, StatusCode};

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

```

You can find a other examples in the [examples directory](https://github.com/reu/touche/tree/master/examples).

## Disclaimer

This library is by no means a critique to Hyper or to async Rust. I **really** love both of then.

The main motivation I had to write this library was to be able to introduce Rust to my co-workers
(which are mainly web developers). A synchronous library is way more beginner friendly than an
async one, and by having an API that ressembles the "canonical" HTTP Rust library, people can
learn Rust concepts in a easier way before adventuring through Hyper and async.
