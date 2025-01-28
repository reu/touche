use std::error::Error;

use touche::{upgrade::Upgrade, Body, Connection, Request, Server};
use tungstenite::{protocol::Role, WebSocket};

fn main() -> std::io::Result<()> {
    Server::bind("0.0.0.0:4444").serve(|req: Request<Body>| {
        let res = tungstenite::handshake::server::create_response(&req.map(|_| ()))?;

        Ok::<_, Box<dyn Error + Send + Sync>>(res.upgrade(|stream: Connection| {
            let mut ws = WebSocket::from_raw_socket(stream, Role::Server, None);

            while let Ok(msg) = ws.read() {
                if msg.is_text() && ws.send(msg).is_err() {
                    break;
                }
            }
        }))
    })
}
