pub mod body;
pub mod request;
pub mod response;
pub mod upgrade;

use std::{
    error::Error,
    io::{self, BufReader, BufWriter},
    net::TcpStream,
};

pub use body::Body;
use response::Outcome;

pub type Request = http::Request<Body>;
pub type Response = http::Response<Body>;

pub trait Handler<Err>: Sync + Send
where
    Err: Into<Box<dyn Error + Send + Sync>>,
{
    fn handle(&self, request: Request) -> Result<Response, Err>;
}

impl<F, Err> Handler<Err> for F
where
    F: Fn(Request) -> Result<Response, Err>,
    F: Sync + Send,
    Err: Into<Box<dyn Error + Send + Sync>>,
{
    fn handle(&self, request: Request) -> Result<Response, Err> {
        self(request)
    }
}

pub enum Connection {
    Close,
    KeepAlive(TcpStream),
}

pub fn serve<Handle, Err>(stream: TcpStream, handle: Handle) -> io::Result<Connection>
where
    Handle: Handler<Err>,
    Err: Into<Box<dyn Error + Send + Sync>>,
{
    let req_stream = BufReader::new(stream.try_clone()?);
    match request::parse_request(req_stream) {
        Ok(req) => {
            let res = handle
                .handle(req)
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

            let mut res_stream = BufWriter::new(stream);

            match response::write_response(res, &mut res_stream)? {
                Outcome::KeepAlive => Ok(Connection::KeepAlive(res_stream.into_inner()?)),
                Outcome::Close => Ok(Connection::Close),
                Outcome::Upgrade(upgrade) => {
                    upgrade.handler.handle(res_stream.into_inner()?);
                    Ok(Connection::Close)
                }
            }
        }
        Err(err) => Err(io::Error::new(io::ErrorKind::Other, err)),
    }
}
