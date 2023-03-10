use std::io::{self, BufReader, BufWriter, Write};

use headers::HeaderMapExt;
use http::StatusCode;

use crate::{request, response, Body, Connection, HttpBody};

pub enum ConnectionOutcome {
    Close,
    KeepAlive(Connection),
    Upgrade(Connection),
}

impl ConnectionOutcome {
    pub fn closed(&self) -> bool {
        matches!(self, ConnectionOutcome::Close)
    }

    pub fn unwrap(self) -> Connection {
        match self {
            ConnectionOutcome::Close => panic!("Connection closed"),
            ConnectionOutcome::KeepAlive(conn) => conn,
            ConnectionOutcome::Upgrade(conn) => conn,
        }
    }

    pub fn into_inner(self) -> Result<Connection, ConnectionOutcome> {
        match self {
            ConnectionOutcome::KeepAlive(conn) => Ok(conn),
            ConnectionOutcome::Upgrade(conn) => Ok(conn),
            ConnectionOutcome::Close => Err(self),
        }
    }
}

pub fn send<C, B>(
    connection: C,
    req: http::Request<B>,
) -> io::Result<(ConnectionOutcome, http::Response<Body>)>
where
    C: Into<Connection>,
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
    } else if res.status() == StatusCode::SWITCHING_PROTOCOLS {
        ConnectionOutcome::Upgrade(writer.into_inner()?)
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

    use http::{Request, Version};

    use crate::Server;

    use super::*;

    #[test]
    fn send_request() {
        let listener = TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        thread::spawn(move || {
            Server::from(listener)
                .serve(|req: Request<_>| http::Response::builder().body(req.into_body()))
                .ok()
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
            Server::from(listener)
                .serve(|_req| {
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
        assert!(conn.closed());
    }

    #[test]
    fn keep_http_10_connection_alive_when_asked_to() {
        let listener = TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        thread::spawn(move || {
            Server::from(listener)
                .serve(|_req| http::Response::builder().body("lolwut"))
                .ok();
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
