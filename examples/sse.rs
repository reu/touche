use std::{error::Error, net::TcpListener, thread, time::Duration};

use http::{Response, StatusCode};
use indoc::indoc;
use shrike::{Body, Request};

fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:4444")?;

    for stream in listener.incoming() {
        let mut stream = stream?;
        thread::spawn(move || {
            shrike::serve(&mut stream, |req: Request| {
                match req
                    .uri()
                    .path()
                    .split("/")
                    .skip(1)
                    .filter(|seg| !seg.is_empty())
                    .collect::<Vec<&str>>()
                    .as_slice()
                {
                    [] => Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "text/html")
                        .body(
                            indoc! {r#"
                            <html>
                              <head>
                                <script>
                                  const evtSource = new EventSource("/sse");

                                  const messages = document.createElement("ul");

                                  evtSource.addEventListener("userconnect", evt => {
                                    const { name } = JSON.parse(evt.data);
                                    messages.insertAdjacentHTML("beforeend", `<li>User ${name} connected`);
                                  });

                                  evtSource.addEventListener("usermessage", evt => {
                                    const { name, message } = JSON.parse(evt.data);
                                    messages.insertAdjacentHTML("beforeend", `<li>${name}: ${message}`);
                                  });

                                  evtSource.addEventListener("userdisconnect", evt => {
                                    const { name } = JSON.parse(evt.data);
                                    messages.insertAdjacentHTML("beforeend", `<li>User ${name} disconnected`);
                                  });

                                  document.addEventListener("DOMContentLoaded", () => {
                                    document.body.appendChild(messages);
                                  });
                                </script>
                              </head>

                              <body></body>
                            </html>
                        "#}
                            .into(),
                        ),

                    ["sse"] => {
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
                        .status(StatusCode::NOT_FOUND)
                        .body(Body::empty()),
                }
            })
        });
    }

    Ok(())
}
