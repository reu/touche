use std::{error::Error, net::TcpStream as StdTcpStream, sync::Arc};

use futures::{stream::StreamExt, SinkExt};
use tokio::{net::TcpStream, runtime};
use tokio_tungstenite::{tungstenite::protocol::Role, WebSocketStream};
use touche::{upgrade::Upgrade, Body, Connection, Request, Server};

fn main() -> std::io::Result<()> {
    let runtime = Arc::new(
        runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap(),
    );

    Server::builder()
        .max_threads(1)
        .bind("0.0.0.0:4444")
        .serve(move |req: Request<Body>| {
            let runtime = runtime.clone();

            let res = tungstenite::handshake::server::create_response(&req.map(|_| ()))?;

            Ok::<_, Box<dyn Error + Send + Sync>>(res.upgrade(move |stream: Connection| {
                let stream = stream.downcast::<StdTcpStream>().unwrap();
                stream.set_nonblocking(true).unwrap();

                runtime.spawn(async move {
                    let stream = TcpStream::from_std(stream).unwrap();
                    let mut ws =
                        WebSocketStream::<TcpStream>::from_raw_socket(stream, Role::Server, None)
                            .await;

                    while let Some(Ok(msg)) = ws.next().await {
                        if msg.is_text() && ws.send(msg).await.is_err() {
                            break;
                        }
                    }
                });
            }))
        })
}
