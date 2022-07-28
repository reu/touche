pub mod body;
pub mod connection;
mod read_queue;
pub mod request;
pub mod response;
pub mod server;
pub mod upgrade;

use std::{
    error::Error,
    io::{self, BufReader, BufWriter, Write},
};

pub use body::Body;
use body::HttpBody;
use connection::Connection;
use headers::{HeaderMapExt, HeaderValue};
use http::{StatusCode, Version};
use read_queue::ReadQueue;
use request::ParseError;
use response::Outcome;
pub use server::Server;

pub type Request = http::Request<Body>;
pub type Response = http::Response<Body>;

pub trait Handler<Body, Err>: Clone
where
    Body: HttpBody,
    Err: Into<Box<dyn Error + Send + Sync>>,
{
    fn handle(&self, request: Request) -> Result<http::Response<Body>, Err>;

    fn should_continue(&self, _: &Request) -> StatusCode {
        StatusCode::CONTINUE
    }
}

impl<F, Body, Err> Handler<Body, Err> for F
where
    F: Fn(Request) -> Result<http::Response<Body>, Err>,
    F: Sync + Send,
    F: Clone,
    Body: HttpBody,
    Err: Into<Box<dyn Error + Send + Sync>>,
{
    fn handle(&self, request: Request) -> Result<http::Response<Body>, Err> {
        self(request)
    }
}

pub fn serve<Conn, Handle, Body, Err>(stream: Conn, handle: Handle) -> io::Result<()>
where
    Conn: Into<Connection>,
    Handle: Handler<Body, Err>,
    Body: HttpBody,
    Err: Into<Box<dyn Error + Send + Sync>>,
{
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
                    match handle.should_continue(&req) {
                        status @ StatusCode::CONTINUE => {
                            let res = http::Response::builder().status(status).body(()).unwrap();
                            response::write_response(res, &mut writer)?;
                            writer.flush()?;
                        }
                        status => {
                            let res = http::Response::builder().status(status).body(()).unwrap();
                            response::write_response(res, &mut writer)?;
                            writer.flush()?;
                            continue;
                        }
                    };
                }

                let mut res = handle
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
