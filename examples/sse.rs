use std::{error::Error, net::TcpListener, thread, time::Duration};

use http::{header::ACCEPT, Response, StatusCode};
use indoc::indoc;
use touche::{Body, Request};

fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:4444")?;

    for stream in listener.incoming() {
        let stream = stream?;
        thread::spawn(|| {
            touche::serve(stream, |req: Request| {
                match req.headers().get(ACCEPT).and_then(|a| a.to_str().ok()) {
                    Some(accept) if accept.contains("text/event-stream") => {
                        let (sender, body) = Body::channel();

                        thread::spawn(move || {
                            sender.send(indoc! {r#"
                                event: userconnect
                                data: {"name": "sasha"}

                            "#})?;

                            for _ in 1..10 {
                                thread::sleep(Duration::from_secs(1));
                                sender.send(indoc! {r#"
                                    event: usermessage
                                    data: {"name": "sasha", "message": "message"}

                                "#})?;
                            }

                            thread::sleep(Duration::from_secs(1));
                            sender.send(indoc! {r#"
                                event: userdisconnect
                                data: {"name": "sasha"}

                            "#})?;

                            Ok::<_, Box<dyn Error + Send + Sync>>(())
                        });

                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "text/event-stream")
                            .body(body)
                    }

                    _ => Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "text/html")
                        .body(include_str!("sse.html").into()),
                }
            })
        });
    }

    Ok(())
}
