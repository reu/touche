# touche

Touché is a low level but fully featured HTTP 1.0/1.1 library.

It tries to mimic [hyper](https://crates.io/crates/hyper), but with a synchronous API.

More information can be found in the [crate documentation](https://docs.rs/touche).

## Hello world example

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

You can find a other examples in the [examples directory](https://github.com/reu/touche/tree/master/examples).

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
