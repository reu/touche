use touche::{
    body::{ChunkIterator, HttpBody},
    Response, Server, StatusCode,
};

use flate2::read::GzEncoder;
use flate2::Compression;

struct Compressed<Body: HttpBody>(Body);

impl<Body> HttpBody for Compressed<Body>
where
    Body: HttpBody,
    Body::Reader: 'static,
{
    type Reader = GzEncoder<Body::Reader>;
    type Chunks = ChunkIterator;

    fn len(&self) -> Option<u64> {
        None
    }

    fn into_reader(self) -> Self::Reader {
        GzEncoder::new(self.0.into_reader(), Compression::fast())
    }

    fn into_chunks(self) -> Self::Chunks {
        ChunkIterator::from_reader(self.into_reader(), None)
    }
}

fn main() -> std::io::Result<()> {
    Server::bind("0.0.0.0:4444").serve(|_req| {
        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/plain")
            .header("content-encoding", "gzip")
            .body(Compressed(include_bytes!("./compress.rs").as_ref()))
    })
}
