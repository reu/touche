use std::{
    collections::HashMap,
    error::Error,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Sender},
        Arc, Mutex,
    },
    thread,
};

use headers::HeaderMapExt;
use serde::{Deserialize, Serialize};
use touche::{upgrade::Upgrade, Body, Connection, Request, Response, Server, StatusCode};
use tungstenite::{protocol::Role, WebSocket};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
enum Event {
    #[serde(rename = "user")]
    User { id: usize },
    #[serde(rename = "message", rename_all = "camelCase")]
    Message { user_id: usize, text: String },
}

type Users = Arc<Mutex<HashMap<usize, Sender<Event>>>>;

fn main() -> std::io::Result<()> {
    static NEXT_USER_ID: AtomicUsize = AtomicUsize::new(1);

    let users: Users = Arc::new(Mutex::new(HashMap::new()));

    Server::bind("0.0.0.0:4444").serve(move |req: Request<Body>| {
        let users = users.clone();

        if req.headers().typed_get::<headers::Upgrade>().is_some() {
            let res = tungstenite::handshake::server::create_response(&req.map(|_| ()))?
                .map(|_| Body::empty());

            Ok::<_, Box<dyn Error + Send + Sync>>(res.upgrade(move |stream: Connection| {
                let users = users.clone();

                let user_id = NEXT_USER_ID.fetch_add(1, Ordering::Relaxed);
                let (tx, rx) = mpsc::channel();

                {
                    let mut users = users.lock().unwrap();

                    users.insert(user_id, tx);

                    let joined_msg = Event::User { id: user_id };
                    users.retain(|_id, tx| tx.send(joined_msg.clone()).is_ok());
                };

                let mut read_ws = WebSocket::from_raw_socket(stream.clone(), Role::Server, None);
                let mut write_ws = WebSocket::from_raw_socket(stream, Role::Server, None);

                let write_ws = thread::spawn(move || {
                    for evt in rx {
                        let msg =
                            tungstenite::Message::Text(serde_json::to_string(&evt).unwrap().into());
                        if write_ws.send(msg).is_err() {
                            break;
                        }
                    }
                });

                let read_ws = thread::spawn(move || {
                    while let Ok(msg) = read_ws.read() {
                        match msg.to_text() {
                            Ok(text) => {
                                let text = text.to_owned();
                                let msg = Event::Message { user_id, text };
                                users
                                    .lock()
                                    .unwrap()
                                    .retain(|_id, tx| tx.send(msg.clone()).is_ok());
                            }
                            Err(_) => break,
                        }
                    }
                });

                write_ws.join().unwrap();
                read_ws.join().unwrap();
            }))
        } else {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/html")
                .body(include_str!("websocket-chat.html").into())?)
        }
    })
}
