use std::{env, net::TcpListener};

use http::{Response, StatusCode};
use shrike::body::{ChunkIterator, HttpBody};
use threadpool::ThreadPool;

use flate2::read::GzEncoder;
use flate2::Compression;

struct Compressed<Body: HttpBody>(Body);

impl<Body> HttpBody for Compressed<Body>
where
    Body: HttpBody,
    Body::BodyReader: 'static,
{
    type BodyReader = GzEncoder<Body::BodyReader>;
    type Chunks = ChunkIterator;

    fn len(&self) -> Option<u64> {
        None
    }

    fn into_reader(self) -> Self::BodyReader {
        GzEncoder::new(self.0.into_reader(), Compression::fast())
    }

    fn into_chunks(self) -> Self::Chunks {
        ChunkIterator::from_reader(self.into_reader(), None)
    }
}

fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:4444")?;

    let threads = match env::var("THREADS") {
        Ok(threads) => threads.parse::<usize>().expect("Invalid THREADS value"),
        Err(_) => 100,
    };

    let pool = ThreadPool::new(threads);

    for stream in listener.incoming() {
        let stream = stream?;
        pool.execute(move || {
            shrike::serve(stream, |_req| {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/plain")
                    .header("content-encoding", "gzip")
                    .body(Compressed(include_bytes!("./compress.rs").as_ref()))
            })
            .ok();
        });
    }

    Ok(())
}
