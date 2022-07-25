pub mod body;
pub mod request;
pub mod response;
pub mod upgrade;

use std::{
    error::Error,
    io::{self, BufReader, BufWriter, Write},
};

pub mod connection;
mod read_queue;

pub use body::Body;
use body::HttpBody;
use connection::Connection;
use headers::HeaderMapExt;
use http::Version;
use read_queue::ReadQueue;
use response::Outcome;

pub type Request = http::Request<Body>;
pub type Response = http::Response<Body>;

pub trait Handler<Body, Err>: Sync + Send
where
    Body: HttpBody,
    Err: Into<Box<dyn Error + Send + Sync>>,
{
    fn handle(&self, request: Request) -> Result<http::Response<Body>, Err>;
}

impl<F, Body, Err> Handler<Body, Err> for F
where
    F: Fn(Request) -> Result<http::Response<Body>, Err>,
    F: Sync + Send,
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

                let demands_close = asks_for_close || req.version() == Version::HTTP_10;

                let res = handle
                    .handle(req)
                    .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

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
            Err(err) => return Err(io::Error::new(io::ErrorKind::Other, err)),
        }
    }

    Ok(())
}
