pub mod request;
pub mod response;

use std::{
    error::Error,
    io::{self, BufReader, BufWriter, Write},
    net::TcpStream,
};

use http::{Request, Response};

pub fn serve<Handle, Err>(stream: &mut TcpStream, handle: Handle) -> io::Result<()>
where
    Handle: Fn(Request<request::Body>) -> Result<Response<response::Body>, Err>,
    Err: Into<Box<dyn Error + Send + Sync>>,
{
    let req_stream = BufReader::new(stream.try_clone()?);
    match request::parse_request(req_stream) {
        Ok(req) => {
            let res = handle(req).map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
            let mut res_stream = BufWriter::new(stream);
            response::write_response(res, &mut res_stream)?;
            res_stream.flush()?;
            Ok(())
        }
        Err(err) => Err(io::Error::new(io::ErrorKind::Other, err)),
    }
}
