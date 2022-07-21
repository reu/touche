pub mod body;
pub mod request;
pub mod response;
pub mod upgrade;

use std::{
    error::Error,
    io::{self, BufReader, BufWriter, Write},
    net::TcpStream,
};

mod read_queue;

pub use body::Body;
use read_queue::ReadQueue;
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

pub fn serve<Handle, Err>(stream: TcpStream, handle: Handle) -> io::Result<()>
where
    Handle: Handler<Err>,
    Err: Into<Box<dyn Error + Send + Sync>>,
{
    let mut read_queue = ReadQueue::new(BufReader::new(stream.try_clone()?));

    let mut reader = read_queue.enqueue();
    let mut writer = BufWriter::new(stream);

    loop {
        match request::parse_request(reader) {
            Ok(req) => {
                reader = read_queue.enqueue();

                let res = handle
                    .handle(req)
                    .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

                match response::write_response(res, &mut writer)? {
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
