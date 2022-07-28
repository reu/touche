use std::{error::Error, thread, time::Duration};

use indoc::indoc;
use touche::{header::ACCEPT, Body, Request, Response, Server, StatusCode};

fn main() -> std::io::Result<()> {
    Server::bind("0.0.0.0:4444").serve(|req: Request<_>| {
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
}
