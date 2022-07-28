use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Sender},
        Arc, Mutex,
    },
    thread,
};

use indoc::formatdoc;
use touche::{body::HttpBody, Body, Method, Request, Response, Server, StatusCode};

#[derive(Debug)]
enum Event {
    User(usize),
    Message(usize, String),
}

type Users = Arc<Mutex<HashMap<usize, Sender<Event>>>>;

fn main() -> std::io::Result<()> {
    static NEXT_USER_ID: AtomicUsize = AtomicUsize::new(1);

    let users: Users = Arc::new(Mutex::new(HashMap::new()));

    Server::bind("0.0.0.0:4444").serve(move |req: Request<Body>| {
        let users = users.clone();

        match req.uri().path() {
            "/messages" if req.method() == Method::POST => {
                let user_id = req
                    .headers()
                    .get("x-user-id")
                    .and_then(|h| h.to_str().ok())
                    .and_then(|user_id| user_id.parse::<usize>().ok());

                match user_id {
                    Some(user_id) => {
                        let text = req.into_body().into_bytes().unwrap_or_default();
                        let text = std::str::from_utf8(&text).unwrap_or_default();

                        users.lock().unwrap().retain(|id, tx| {
                            if user_id != *id {
                                tx.send(Event::Message(user_id, text.to_string())).is_ok()
                            } else {
                                true
                            }
                        });

                        Response::builder()
                            .status(StatusCode::CREATED)
                            .body(Body::empty())
                    }

                    None => Response::builder()
                        .status(StatusCode::UNAUTHORIZED)
                        .body("Missing x-user-id header".into()),
                }
            }

            "/messages" => {
                let user_id = NEXT_USER_ID.fetch_add(1, Ordering::Relaxed);

                let (tx, rx) = mpsc::channel();
                tx.send(Event::User(user_id)).unwrap();
                users.lock().unwrap().insert(user_id, tx);

                let (sender, body) = Body::channel();
                thread::spawn(move || {
                    for event in rx {
                        let message = match event {
                            Event::User(id) => formatdoc! {r#"
                                event: user
                                data: {{"id": "{id}"}}

                            "#},
                            Event::Message(user_id, text) => formatdoc! {r#"
                                event: message
                                data: {{"userId": {user_id}, "message": "{text}"}}

                            "#},
                        };

                        if let Err(_err) = sender.send(message) {
                            break;
                        }
                    }
                });

                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/event-stream")
                    .header("connection", "close")
                    .body(body)
            }

            "/" => Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/html")
                .body(include_str!("sse-chat.html").into()),

            _ => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::empty()),
        }
    })
}
