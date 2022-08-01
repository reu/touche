# touché

Touché is a low level but fully featured HTTP 1.0/1.1 library.

It tries to mimic [hyper](https://crates.io/crates/hyper), but with a synchronous API.

For now only the server API is implemented.

## Hello world

```rust no_run
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
- HTTP Server (thread per connection model, backed by a thread pool)
- Non buffered (streaming) requests and response bodies
- HTTP/1.1 pipelining
- TLS
- Upgrade connections
- Trailers headers
- 100 continue expectation
- Unix sockets servers

## Comparison with Hyper

Touché shares a lot of similarities with Hyper:

- "Low level"
- Uses the [http crate](https://crates.io/crates/http) to represent HTTP related types
- Allows fine-grained implementations of streaming HTTP bodies

But also has some key differences:

- It is synchronous
- Uses `Vec<u8>` to represent bytes instead of [Bytes](https://crates.io/crates/bytes)
- Doesn't support HTTP 2 (and probably never will)

## Other examples

### Chunked response

```rust no_run
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

### Streaming files

```rust no_run
use std::{fs, io};

use touche::{Body, Response, Server, StatusCode};

fn main() -> std::io::Result<()> {
    Server::bind("0.0.0.0:4444").serve(|_req| {
        let file = fs::File::open("./examples/file.rs")?;
        Ok::<_, io::Error>(
            Response::builder()
                .status(StatusCode::OK)
                .body(Body::try_from(file)?)
                .unwrap(),
        )
    })
}
```

### Naive routing with pattern matching

```rust no_run
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

### Response upgrades
```rust no_run
use std::io::{BufRead, BufReader, BufWriter, Write};

use touche::{header, upgrade::Upgrade, Connection, Response, Server, StatusCode};

fn main() -> std::io::Result<()> {
    Server::bind("0.0.0.0:4444").serve(|_req| {
        Response::builder()
            .status(StatusCode::SWITCHING_PROTOCOLS)
            .header(header::UPGRADE, "line-protocol")
            .upgrade(|stream: Connection| {
                let reader = BufReader::new(stream.clone());
                let mut writer = BufWriter::new(stream);

                // Just a simple protocol that will echo every line sent
                for line in reader.lines() {
                    match line {
                        Ok(line) if line.as_str() == "quit" => break,
                        Ok(line) => {
                            writer.write_all(format!("{line}\n").as_bytes());
                            writer.flush();
                        }
                        Err(_err) => break,
                    };
                }
            })
            .body("Upgrading...\n")
    })
}
```

You can find other examples in the [examples directory](https://github.com/reu/touche/tree/master/examples).

## Performance

While the primary focus is having a simple and readable implementation, the library
shows some decent performance.

A simple benchmark of the hello_world.rs example gives the following result:

```sh
$ cat /proc/cpuinfo | grep name | uniq
model name      : AMD Ryzen 5 5600G with Radeon Graphics

$ wrk --latency -t6 -c 200 -d 10s http://localhost:4444
Running 10s test @ http://localhost:4444
  6 threads and 200 connections
  Thread Stats   Avg      Stdev     Max   +/- Stdev
    Latency   153.37us  391.20us  19.41ms   99.37%
    Req/Sec    76.11k    13.21k   89.14k    82.67%
  Latency Distribution
     50%  126.00us
     75%  160.00us
     90%  209.00us
     99%  360.00us
  4544074 requests in 10.01s, 225.35MB read
Requests/sec: 454157.11
Transfer/sec:     22.52MB
```

The result is on par with Hyper's hello world running on the same machine.

## Disclaimer

This library is by no means a critique to Hyper or to async Rust. I **really** love both of them.

The main motivation I had to write this library was to be able to introduce Rust to my co-workers
(which are mainly web developers). A synchronous library is way more beginner friendly than an
async one, and by having an API that ressembles the "canonical" HTTP Rust library, people can
learn Rust concepts in a easier way before adventuring through Hyper and async.
