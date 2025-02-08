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
- A simple and easy to read implementation and examples

But also has some key differences:

- It is synchronous
- Uses `Vec<u8>` to represent bytes instead of [Bytes](https://crates.io/crates/bytes)
- Doesn't support HTTP 2 (and probably never will)

## Handling persistent connections with non blocking IO

Connection-per-thread web servers are notorious bad with persistent connections like websockets or event streams.
This is primarily because the thread gets locked to the connection until it is closed.

One solution to this problem is to handle such connections with non-blocking IO.
By doing so, the server thread becomes available for other connections.

The following example demonstrates a single-threaded touché server that handles websockets upgrades to a Tokio runtime.

```rust no_run
use std::error::Error;

use futures::{stream::StreamExt, SinkExt};
use tokio::{net::TcpStream, runtime};
use tokio_tungstenite::{tungstenite::protocol::Role, WebSocketStream};
use touche::{upgrade::Upgrade, Body, Connection, Request, Server};

fn main() -> std::io::Result<()> {
    let tokio_runtime = runtime::Builder::new_multi_thread().enable_all().build()?;
    let tokio_handle = tokio_runtime.handle();

    Server::builder()
        .bind("0.0.0.0:4444")
        // We can have can handle multiple websockets even with a single thread server, since the
        // websocket connections will be handled by Tokio and not by Touche.
        .serve_single_thread(move |req: Request<Body>| {
            let tokio_handle = tokio_handle.clone();

            let res = tungstenite::handshake::server::create_response(&req.map(|_| ()))?;

            Ok::<_, Box<dyn Error + Send + Sync>>(res.upgrade(move |stream: Connection| {
                let stream = stream.downcast::<std::net::TcpStream>().unwrap();
                stream.set_nonblocking(true).unwrap();

                tokio_handle.spawn(async move {
                    let stream = TcpStream::from_std(stream).unwrap();
                    let mut ws = WebSocketStream::from_raw_socket(stream, Role::Server, None).await;

                    while let Some(Ok(msg)) = ws.next().await {
                        if msg.is_text() && ws.send(msg).await.is_err() {
                            break;
                        }
                    }
                });
            }))
        })
}
```

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

use touche::{header, upgrade::Upgrade, Body, Connection, Response, Server, StatusCode};

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
            .body(Body::empty())
    })
}
```

You can find other examples in the [examples directory](https://github.com/reu/touche/tree/master/examples).

## Performance

While the primary focus is having a simple and readable implementation, the library
shows some decent performance.

A simple benchmark of the hello-world.rs example gives the following result:

```sh
$ cat /proc/cpuinfo | grep name | uniq
model name      : 13th Gen Intel(R) Core(TM) i7-13700K

$ wrk --latency -t6 -c 200 -d 10s http://localhost:4444
Running 10s test @ http://localhost:4444
  6 threads and 200 connections
  Thread Stats   Avg      Stdev     Max   +/- Stdev
    Latency    51.73us  450.03us  29.59ms   99.81%
    Req/Sec   251.58k    52.42k  366.43k    70.15%
  Latency Distribution
     50%   36.00us
     75%   47.00us
     90%   71.00us
     99%  115.00us
  15089728 requests in 10.10s, 1.25GB read
Requests/sec: 1494130.41
Transfer/sec:    126.82MB
```

The result is on par with Hyper's hello world running on the same machine.

## Disclaimer

The main motivation I had to write this library was to be able to introduce Rust to my co-workers
(which are mainly web developers).

Most of HTTP server libraries in Rust are async, which makes sense for the problem domain, but with
that some additional complexity comes together, which can be overwhelming when you are just getting
start with the language.

The ideia is to provide an API that ressembles the "canonical" HTTP Rust library, so people can
learn its concepts in a easier way before adventuring through Hyper and async Rust.
