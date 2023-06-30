//! Streaming bodies for [`Requests`](http::Request) and [`Responses`](http::Response).
//!
//! Bodies are not buffered by default, so applications don't use memory they don't need.
//!
//! As [hyper](https://docs.rs/hyper) this module has two important pieces:
//!
//! - The [`HttpBody`] trait, which describes all possible bodies. This allows custom
//!   implementation if you need fine-grained control on how to stream and chunk the data.
//! - The [`Body`] concrete type, which is an implementation of [`HttpBody`] returned by touche
//!   as a "receive stream". It is also a decent default implementation for your send streams.
use std::{
    error::Error,
    fs::File,
    io::{self, Cursor, Read},
    sync::mpsc::{self, Sender},
};

use headers::{HeaderMap, HeaderName, HeaderValue};
pub use http_body::*;

mod http_body;

/// The [`HttpBody`] used on receiving server requests.
/// It is also a good default body to return as responses.
#[derive(Default)]
pub struct Body(Option<BodyInner>);

#[derive(Default)]
enum BodyInner {
    #[default]
    Empty,
    Buffered(Vec<u8>),
    Iter(Box<dyn Iterator<Item = io::Result<Chunk>> + Send>),
    Reader(Box<dyn Read + Send>, Option<usize>),
}

/// The sender half of a channel, used to stream chunks from another thread.
pub struct BodyChannel(Sender<io::Result<Chunk>>);

impl BodyChannel {
    /// Send a chunk of bytes to this body.
    pub fn send<T: Into<Vec<u8>>>(&self, data: T) -> io::Result<()> {
        self.0
            .send(Ok(data.into().into()))
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "body closed"))
    }

    /// Send a trailer header. Note that trailers will be buffered, so you are not required to send
    /// them only after sending all the chunks.
    pub fn send_trailer<K, V>(
        &self,
        header: K,
        value: V,
    ) -> Result<(), Box<dyn Error + Send + Sync>>
    where
        K: TryInto<HeaderName>,
        V: TryInto<HeaderValue>,
        <K as TryInto<headers::HeaderName>>::Error: Error + Send + Sync + 'static,
        <V as TryInto<headers::HeaderValue>>::Error: Error + Send + Sync + 'static,
    {
        let mut trailers = HeaderMap::new();
        trailers.insert(header.try_into()?, value.try_into()?);
        Ok(self.send_trailers(trailers)?)
    }

    /// Sends trailers to this body. Not that trailers will be buffered, so you are not required to
    /// send then only after sending all the chunks.
    pub fn send_trailers(&self, trailers: HeaderMap) -> io::Result<()> {
        self.0
            .send(Ok(Chunk::Trailers(trailers)))
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "body closed"))
    }

    /// Aborts the body in an abnormal fashion.
    pub fn abort(self) {
        self.0
            .send(Err(io::Error::new(io::ErrorKind::Other, "aborted")))
            .ok();
    }
}

impl Body {
    /// Creates an empty [`Body`] stream.
    pub fn empty() -> Self {
        Body(Some(BodyInner::Empty))
    }

    /// Creates a [`Body`] stream with an associated sender half.
    /// Useful when wanting to stream chunks from another thread.
    pub fn channel() -> (BodyChannel, Self) {
        let (tx, rx) = mpsc::channel();
        let body = Body(Some(BodyInner::Iter(Box::new(rx.into_iter()))));
        (BodyChannel(tx), body)
    }

    /// Creates a [`Body`] stream from an Iterator of chunks.
    /// Each item emitted will be written as a separated chunk on chunked encoded requests or
    /// responses.
    #[allow(clippy::should_implement_trait)]
    pub fn from_iter<T, I>(chunks: I) -> Self
    where
        T: Into<Chunk>,
        I: IntoIterator<Item = T> + Send + 'static,
        <I as IntoIterator>::IntoIter: Send,
    {
        Body(Some(BodyInner::Iter(Box::new(
            chunks.into_iter().map(|chunk| Ok(chunk.into())),
        ))))
    }

    /// Creates a [`Body`] stream from an [`Read`], with an optional length.
    pub fn from_reader<T: Into<Option<usize>>>(
        reader: impl Read + Send + 'static,
        length: T,
    ) -> Self {
        Body(Some(BodyInner::Reader(Box::new(reader), length.into())))
    }
}

impl HttpBody for Body {
    type Reader = BodyReader;
    type Chunks = ChunkIterator;

    fn len(&self) -> Option<u64> {
        match &self.0 {
            Some(BodyInner::Empty) => Some(0),
            Some(BodyInner::Buffered(bytes)) => Some(bytes.len() as u64),
            Some(BodyInner::Iter(_)) => None,
            Some(BodyInner::Reader(_, Some(len))) => Some(*len as u64),
            Some(BodyInner::Reader(_, None)) => None,
            None => None,
        }
    }

    fn into_reader(mut self) -> Self::Reader {
        match self.0.take().unwrap() {
            BodyInner::Empty => BodyReader(BodyReaderInner::Buffered(Cursor::new(Vec::new()))),
            BodyInner::Buffered(bytes) => BodyReader(BodyReaderInner::Buffered(Cursor::new(bytes))),
            BodyInner::Iter(chunks) => {
                let mut chunks = chunks.filter_map(|chunk| match chunk {
                    Ok(Chunk::Data(data)) => Some(Ok(data)),
                    Ok(Chunk::Trailers(_)) => None,
                    Err(err) => Some(Err(err)),
                });
                let cursor = chunks
                    .next()
                    .map(|chunk| chunk.unwrap_or_default())
                    .map(Cursor::new);
                BodyReader(BodyReaderInner::Iter(Box::new(chunks), cursor))
            }
            BodyInner::Reader(stream, Some(len)) => {
                BodyReader(BodyReaderInner::Reader(Box::new(stream.take(len as u64))))
            }
            BodyInner::Reader(stream, None) => BodyReader(BodyReaderInner::Reader(stream)),
        }
    }

    fn into_bytes(mut self) -> io::Result<Vec<u8>> {
        match self.0.take().unwrap() {
            BodyInner::Empty => Ok(Vec::new()),
            BodyInner::Buffered(bytes) => Ok(bytes),
            BodyInner::Iter(chunks) => Ok(chunks
                .filter_map(|chunk| match chunk {
                    Ok(Chunk::Data(data)) => Some(Ok(data)),
                    Ok(Chunk::Trailers(_)) => None,
                    Err(err) => Some(Err(err)),
                })
                .collect::<io::Result<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect()),
            BodyInner::Reader(stream, Some(len)) => {
                let mut buf = Vec::with_capacity(len);
                stream.take(len as u64).read_to_end(&mut buf)?;
                Ok(buf)
            }
            BodyInner::Reader(mut stream, None) => {
                let mut buf = Vec::with_capacity(8 * 1024);
                stream.read_to_end(&mut buf)?;
                Ok(buf)
            }
        }
    }

    fn into_chunks(mut self) -> Self::Chunks {
        match self.0.take().unwrap() {
            BodyInner::Empty => ChunkIterator(None),
            BodyInner::Buffered(bytes) => ChunkIterator(Some(ChunkIteratorInner::Single(bytes))),
            BodyInner::Iter(chunks) => ChunkIterator(Some(ChunkIteratorInner::Iter(chunks))),
            BodyInner::Reader(reader, len) => {
                ChunkIterator(Some(ChunkIteratorInner::Reader(reader, len)))
            }
        }
    }
}

impl Drop for Body {
    fn drop(&mut self) {
        #[allow(unused_must_use)]
        match self.0.take() {
            Some(BodyInner::Reader(ref mut stream, Some(len))) => {
                io::copy(&mut stream.take(len as u64), &mut io::sink());
            }
            Some(BodyInner::Reader(ref mut stream, None)) => {
                io::copy(stream, &mut io::sink());
            }
            _ => {}
        }
    }
}

impl From<Vec<u8>> for Body {
    fn from(body: Vec<u8>) -> Self {
        Body(Some(BodyInner::Buffered(body)))
    }
}

impl From<&[u8]> for Body {
    fn from(body: &[u8]) -> Self {
        body.to_vec().into()
    }
}

impl From<&str> for Body {
    fn from(body: &str) -> Self {
        body.as_bytes().to_vec().into()
    }
}

impl From<String> for Body {
    fn from(body: String) -> Self {
        body.into_bytes().into()
    }
}

impl TryFrom<File> for Body {
    type Error = io::Error;

    fn try_from(file: File) -> Result<Self, Self::Error> {
        match file.metadata() {
            Ok(meta) if meta.is_file() => Ok(Body::from_reader(file, meta.len() as usize)),
            Ok(_) => Err(io::Error::new(io::ErrorKind::Other, "not a file")),
            Err(err) => Err(err),
        }
    }
}

/// Wraps a body and turns into a [`Read`].
pub struct BodyReader(BodyReaderInner);

impl BodyReader {
    /// Creates a [`BodyReader`] from an [`Read`]
    pub fn from_reader(reader: impl Read + 'static) -> Self {
        BodyReader(BodyReaderInner::Reader(Box::new(reader)))
    }

    /// Creates a [`BodyReader`] from an [`Iterator`]
    #[allow(clippy::should_implement_trait)]
    pub fn from_iter(iter: impl IntoIterator<Item = Vec<u8>> + 'static) -> Self {
        let mut iter = iter.into_iter();
        let cursor = iter.next().map(Cursor::new);
        BodyReader(BodyReaderInner::Iter(Box::new(iter.map(Ok)), cursor))
    }
}

enum BodyReaderInner {
    Buffered(Cursor<Vec<u8>>),
    Iter(
        Box<dyn Iterator<Item = io::Result<Vec<u8>>>>,
        Option<Cursor<Vec<u8>>>,
    ),
    Reader(Box<dyn Read>),
}

impl Read for BodyReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.0 {
            BodyReaderInner::Buffered(ref mut cursor) => cursor.read(buf),
            BodyReaderInner::Reader(ref mut reader) => reader.read(buf),

            // TODO: support for non partial reads here
            BodyReaderInner::Iter(ref mut iter, ref mut leftover) => {
                while let Some(ref mut cursor) = leftover {
                    let read = cursor.read(buf)?;
                    if read > 0 {
                        return Ok(read);
                    }
                    let next = iter.next().and_then(|next| next.ok()).map(Cursor::new);
                    *leftover = next;
                }
                Ok(0)
            }
        }
    }
}

impl From<Vec<u8>> for BodyReader {
    fn from(buf: Vec<u8>) -> Self {
        Self(BodyReaderInner::Buffered(Cursor::new(buf)))
    }
}

impl From<Body> for BodyReader {
    fn from(mut body: Body) -> Self {
        match body.0.take().unwrap() {
            BodyInner::Empty => Vec::new().into(),
            BodyInner::Buffered(bytes) => bytes.into(),
            BodyInner::Iter(chunks) => {
                let mut chunks = chunks.filter_map(|chunk| match chunk {
                    Ok(Chunk::Data(data)) => Some(Ok(data)),
                    Ok(Chunk::Trailers(_)) => None,
                    Err(err) => Some(Err(err)),
                });
                let cursor = chunks
                    .next()
                    .map(|chunk| chunk.unwrap_or_default())
                    .map(Cursor::new);
                BodyReader(BodyReaderInner::Iter(Box::new(chunks), cursor))
            }
            BodyInner::Reader(stream, Some(len)) => {
                BodyReader(BodyReaderInner::Reader(Box::new(stream.take(len as u64))))
            }
            BodyInner::Reader(stream, None) => BodyReader(BodyReaderInner::Reader(stream)),
        }
    }
}

/// Iterate bodies in chunks
pub struct ChunkIterator(Option<ChunkIteratorInner>);

impl ChunkIterator {
    pub fn from_reader<T: Into<Option<usize>>>(reader: impl Read + 'static, length: T) -> Self {
        Self(Some(ChunkIteratorInner::Reader(
            Box::new(reader),
            length.into(),
        )))
    }
}

enum ChunkIteratorInner {
    Single(Vec<u8>),
    Iter(Box<dyn Iterator<Item = io::Result<Chunk>>>),
    Reader(Box<dyn Read>, Option<usize>),
}

impl Iterator for ChunkIterator {
    type Item = io::Result<Chunk>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.0.take()? {
            ChunkIteratorInner::Single(bytes) => Some(Ok(bytes.into())),
            ChunkIteratorInner::Iter(mut iter) => {
                let item = iter.next()?.ok()?;
                self.0 = Some(ChunkIteratorInner::Iter(iter));
                Some(Ok(item))
            }
            ChunkIteratorInner::Reader(mut reader, Some(len)) => {
                let mut buf = [0_u8; 8 * 1024];
                match reader.read(&mut buf) {
                    Ok(0) => None,
                    Ok(bytes) => {
                        self.0 = match len.checked_sub(bytes) {
                            r @ Some(rem) if rem > 0 => Some(ChunkIteratorInner::Reader(reader, r)),
                            _ => None,
                        };
                        Some(Ok(buf[0..bytes].to_vec().into()))
                    }
                    Err(err) => Some(Err(err)),
                }
            }
            ChunkIteratorInner::Reader(mut reader, None) => {
                let mut buf = [0_u8; 8 * 1024];
                match reader.read(&mut buf) {
                    Ok(0) => None,
                    Ok(bytes) => {
                        self.0 = Some(ChunkIteratorInner::Reader(reader, None));
                        Some(Ok(buf[0..bytes].to_vec().into()))
                    }
                    Err(err) => Some(Err(err)),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read};

    use crate::{body::HttpBody, Body};

    #[test]
    fn test_body_reader_buffered() {
        let body = Body::from(vec![1_u8, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        let mut reader = body.into_reader();

        let mut buf = [0_u8; 4];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(buf, [1, 2, 3, 4]);

        let mut buf = [0_u8; 1];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(buf, [5]);

        let mut buf = [0_u8; 5];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(buf, [6, 7, 8, 9, 10]);
    }

    #[test]
    fn test_body_reader_chunked() {
        let body = Body::from_iter([vec![1, 2, 3], vec![4, 5, 6], vec![7], vec![8, 9], vec![10]]);
        let mut reader = body.into_reader();

        let mut buf = [0_u8; 4];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(buf, [1, 2, 3, 4]);

        let mut buf = [0_u8; 1];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(buf, [5]);

        let mut buf = [0_u8; 5];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(buf, [6, 7, 8, 9, 10]);
    }

    #[test]
    fn test_body_reader_with_unknown_size() {
        let reader = Cursor::new(vec![1_u8, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        let body = Body::from_reader(reader, None);
        let mut reader = body.into_reader();

        let mut buf = [0_u8; 4];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(buf, [1, 2, 3, 4]);

        let mut buf = [0_u8; 1];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(buf, [5]);

        let mut buf = [0_u8; 5];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(buf, [6, 7, 8, 9, 10]);
    }

    #[test]
    fn test_body_reader_with_known_size() {
        let reader = Cursor::new(vec![1_u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]);
        let body = Body::from_reader(reader, 10);
        let mut reader = body.into_reader();

        let mut buf = [0_u8; 4];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(buf, [1, 2, 3, 4]);

        let mut buf = [0_u8; 1];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(buf, [5]);

        let mut buf = [0_u8; 5];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(buf, [6, 7, 8, 9, 10]);

        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).unwrap();
        assert!(buf.is_empty());
    }

    #[test]
    fn test_chunk_with_errors() {
        let (channel, body) = Body::channel();
        channel.send("123").unwrap();
        channel.send("456").unwrap();
        drop(channel);
        assert_eq!(body.into_bytes().unwrap(), b"123456");

        let (channel, body) = Body::channel();
        channel.send("123").unwrap();
        channel.send("456").unwrap();
        channel.abort();
        assert!(body.into_bytes().is_err());
    }
}
