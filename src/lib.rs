pub mod body;
pub mod connection;
mod read_queue;
pub mod request;
pub mod response;
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

pub type Request = http::Request<Body>;
pub type Response = http::Response<Body>;

pub trait Handler<Body, Err>
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

pub enum ConnectionOutcome {
    Close,
    KeepAlive(Connection),
}

impl ConnectionOutcome {
    pub fn unwrap(self) -> Connection {
        match self {
            ConnectionOutcome::KeepAlive(conn) => conn,
            ConnectionOutcome::Close => panic!("Connection closed"),
        }
    }
}

pub fn send<Conn, B>(
    connection: Conn,
    req: http::Request<B>,
) -> io::Result<(ConnectionOutcome, http::Response<Body>)>
where
    Conn: Into<Connection>,
    B: HttpBody,
{
    let conn = connection.into();

    let reader = BufReader::new(conn.clone());
    let mut writer = BufWriter::new(conn);

    request::write_request(req, &mut writer)?;
    writer.flush()?;

    let res = response::parse_response(reader)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

    let asks_for_close = res
        .headers()
        .typed_get::<headers::Connection>()
        .filter(|conn| conn.contains("close"))
        .is_some();

    let outcome = if asks_for_close {
        ConnectionOutcome::Close
    } else {
        ConnectionOutcome::KeepAlive(writer.into_inner()?)
    };

    Ok((outcome, res))
}

#[cfg(test)]
mod tests {
    use std::{
        io::Cursor,
        net::{TcpListener, TcpStream},
        thread,
    };

    use super::*;

    #[test]
    fn send_request() {
        let listener = TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        thread::spawn(move || {
            let (conn, _) = listener.accept().unwrap();
            serve(conn, |req: Request| {
                http::Response::builder().body(req.into_body())
            })
            .ok();
        });

        let conn = TcpStream::connect(("0.0.0.0", port)).unwrap();

        let req = http::Request::builder().body("Hello world").unwrap();
        let (conn, res) = send(conn, req).unwrap();
        assert_eq!(res.into_body().into_bytes().unwrap(), b"Hello world");

        let req = http::Request::builder().body("Bye world").unwrap();
        let (conn, res) = send(conn.unwrap(), req).unwrap();
        assert_eq!(res.into_body().into_bytes().unwrap(), b"Bye world");

        let req = http::Request::builder().body(()).unwrap();
        let (conn, res) = send(conn.unwrap(), req).unwrap();
        assert_eq!(res.into_body().into_bytes().unwrap(), b"");

        let req = http::Request::builder()
            .header("transfer-encoding", "chunked")
            .body(Body::from_iter(vec![&b"lol"[..], &b"wut"[..]]))
            .unwrap();
        let (_conn, res) = send(conn.unwrap(), req).unwrap();
        assert_eq!(res.into_body().into_bytes().unwrap(), b"lolwut");
    }

    #[test]
    fn correctly_handles_closing_connections() {
        let listener = TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        thread::spawn(move || {
            let (conn, _) = listener.accept().unwrap();
            serve(conn, |_req| {
                http::Response::builder()
                    .header("connection", "close")
                    .body(Body::from_reader(Cursor::new(b"lolwut"), None))
            })
            .ok();
        });

        let conn = TcpStream::connect(("0.0.0.0", port)).unwrap();

        let req = http::Request::builder().body(()).unwrap();
        let (conn, res) = send(conn, req).unwrap();

        assert_eq!(res.into_body().into_bytes().unwrap(), b"lolwut");
        assert!(matches!(conn, ConnectionOutcome::Close));
    }

    #[test]
    fn keep_http_10_connection_alive_when_asked_to() {
        let listener = TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        thread::spawn(move || {
            let (conn, _) = listener.accept().unwrap();
            serve(conn, |_req| http::Response::builder().body("lolwut")).unwrap();
        });

        let conn = TcpStream::connect(("0.0.0.0", port)).unwrap();

        let req = http::Request::builder()
            .version(Version::HTTP_10)
            .header("connection", "keep-alive")
            .body(())
            .unwrap();

        let (conn, res) = send(conn, req).unwrap();

        assert_eq!(res.into_body().into_bytes().unwrap(), b"lolwut");
        assert!(matches!(conn, ConnectionOutcome::KeepAlive(_)));

        let req = http::Request::builder()
            .version(Version::HTTP_10)
            .body(())
            .unwrap();

        let (conn, res) = send(conn.unwrap(), req).unwrap();

        assert_eq!(res.into_body().into_bytes().unwrap(), b"lolwut");
        assert!(matches!(conn, ConnectionOutcome::Close));
    }
}
