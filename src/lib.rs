#![doc = include_str!("../README.md")]

pub mod body;
mod connection;
mod read_queue;
mod request;
mod response;
pub mod server;
#[cfg(feature = "rustls")]
mod tls;
pub mod upgrade;

use std::{
    error::Error,
    io::{self, BufReader, BufWriter, Write},
};

pub use body::Body;
use body::HttpBody;
pub use connection::Connection;
use headers::{HeaderMapExt, HeaderValue};
pub use http::{header, Method, Request, Response, StatusCode, Uri, Version};
use read_queue::ReadQueue;
use request::ParseError;
use response::Outcome;
pub use server::Server;

type IncomingRequest = Request<Body>;

pub trait App {
    type Body: HttpBody;
    type Error: Into<Box<dyn Error + Send + Sync>>;

    fn handle(&self, request: IncomingRequest) -> Result<Response<Self::Body>, Self::Error>;

    fn should_continue(&self, _: &IncomingRequest) -> StatusCode {
        StatusCode::CONTINUE
    }
}

impl<F, Body, Err> App for F
where
    F: Fn(IncomingRequest) -> Result<Response<Body>, Err>,
    F: Sync + Send,
    F: Clone,
    Body: HttpBody,
    Err: Into<Box<dyn Error + Send + Sync>>,
{
    type Body = Body;
    type Error = Err;

    fn handle(&self, request: IncomingRequest) -> Result<Response<Self::Body>, Self::Error> {
        self(request)
    }
}

pub(crate) fn serve<C: Into<Connection>, A: App>(stream: C, app: A) -> io::Result<()> {
    let conn = stream.into();
    let mut read_queue = ReadQueue::new(BufReader::new(conn.clone()));

    let mut reader = read_queue.enqueue();
    let mut writer = BufWriter::new(conn);

    loop {
        match request::parse_request(reader) {
            Ok(req) => {
                reader = read_queue.enqueue();

                let asks_for_close = req
                    .headers()
                    .typed_get::<headers::Connection>()
                    .filter(|conn| conn.contains("close"))
                    .is_some();

                let asks_for_keep_alive = req
                    .headers()
                    .typed_get::<headers::Connection>()
                    .filter(|conn| conn.contains("keep-alive"))
                    .is_some();

                let version = req.version();

                let demands_close = match version {
                    Version::HTTP_09 => true,
                    Version::HTTP_10 => !asks_for_keep_alive,
                    _ => asks_for_close,
                };

                let expects_continue = req
                    .headers()
                    .typed_get::<headers::Expect>()
                    .filter(|expect| expect == &headers::Expect::CONTINUE)
                    .is_some();

                if expects_continue {
                    match app.should_continue(&req) {
                        status @ StatusCode::CONTINUE => {
                            let res = Response::builder().status(status).body(()).unwrap();
                            response::write_response(res, &mut writer)?;
                            writer.flush()?;
                        }
                        status => {
                            let res = Response::builder().status(status).body(()).unwrap();
                            response::write_response(res, &mut writer)?;
                            writer.flush()?;
                            continue;
                        }
                    };
                }

                let mut res = app
                    .handle(req)
                    .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

                *res.version_mut() = version;

                if version == Version::HTTP_10 && !asks_for_keep_alive {
                    res.headers_mut()
                        .insert("connection", HeaderValue::from_static("close"));
                }

                match response::write_response(res, &mut writer)? {
                    Outcome::KeepAlive if demands_close => break,
                    Outcome::KeepAlive => writer.flush()?,
                    Outcome::Close => break,
                    Outcome::Upgrade(upgrade) => {
                        upgrade.handler.handle(writer.into_inner()?);
                        break;
                    }
                }
            }
            Err(ParseError::ConnectionClosed) => break,
            Err(err) => return Err(io::Error::new(io::ErrorKind::Other, err)),
        }
    }

    Ok(())
}
