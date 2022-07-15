pub mod request;
pub mod response;

use std::{
    error::Error,
    io::{self, BufReader, BufWriter, Write},
    net::TcpStream,
};

pub use request::Body as RequestBody;
pub use response::Body as ResponseBody;

pub type Request = http::Request<RequestBody>;
pub type Response = http::Response<ResponseBody>;

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

pub fn serve<Handle, Err>(stream: &mut TcpStream, handle: Handle) -> io::Result<()>
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
            response::write_response(res, &mut res_stream)?;
            res_stream.flush()?;
            Ok(())
        }
        Err(err) => Err(io::Error::new(io::ErrorKind::Other, err)),
    }
}
