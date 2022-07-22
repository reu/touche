use std::{
    fs::File,
    io::{self, Cursor, Read},
    sync::mpsc::{self, SendError, Sender},
};

#[derive(Default)]
pub struct Body(Option<BodyInner>);

#[derive(Default)]
enum BodyInner {
    #[default]
    Empty,
    Buffered(Vec<u8>),
    Iter(Box<dyn Iterator<Item = Vec<u8>>>),
    Reader(Box<dyn Read>, Option<usize>),
}

pub struct BodyChannel(Sender<Vec<u8>>);

impl BodyChannel {
    pub fn send<T: Into<Vec<u8>>>(&self, data: T) -> Result<(), SendError<Vec<u8>>> {
        self.0.send(data.into())
    }
}

impl Body {
    pub fn empty() -> Self {
        Body(Some(BodyInner::Empty))
    }

    pub fn channel() -> (BodyChannel, Self) {
        let (tx, rx) = mpsc::channel();
        let body = Body(Some(BodyInner::Iter(Box::new(rx.into_iter()))));
        (BodyChannel(tx), body)
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_iter<T: Into<Vec<u8>>>(chunks: impl IntoIterator<Item = T> + 'static) -> Self {
        Body(Some(BodyInner::Iter(Box::new(
            chunks.into_iter().map(|chunk| chunk.into()),
        ))))
    }

    pub fn from_reader<T: Into<Option<usize>>>(reader: impl Read + 'static, length: T) -> Self {
        Body(Some(BodyInner::Reader(Box::new(reader), length.into())))
    }

    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> Option<u64> {
        match &self.0 {
            Some(BodyInner::Empty) => Some(0),
            Some(BodyInner::Buffered(bytes)) => Some(bytes.len() as u64),
            Some(BodyInner::Iter(_)) => None,
            Some(BodyInner::Reader(_, Some(len))) => Some(*len as u64),
            Some(BodyInner::Reader(_, None)) => None,
            None => None,
        }
    }

    pub fn into_bytes(mut self) -> io::Result<Vec<u8>> {
        match self.0.take().unwrap() {
            BodyInner::Empty => Ok(Vec::new()),
            BodyInner::Buffered(bytes) => Ok(bytes),
            BodyInner::Iter(chunks) => Ok(chunks.flatten().collect()),
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

    pub fn into_reader(mut self) -> impl Read {
        match self.0.take().unwrap() {
            BodyInner::Empty => BodyReader(BodyReaderInner::Buffered(Cursor::new(Vec::new()))),
            BodyInner::Buffered(bytes) => BodyReader(BodyReaderInner::Buffered(Cursor::new(bytes))),
            BodyInner::Iter(mut chunks) => {
                let cursor = chunks.next().map(Cursor::new);
                BodyReader(BodyReaderInner::Iter(chunks, cursor))
            }
            BodyInner::Reader(stream, Some(len)) => {
                BodyReader(BodyReaderInner::Reader(Box::new(stream.take(len as u64))))
            }
            BodyInner::Reader(stream, None) => BodyReader(BodyReaderInner::Reader(stream)),
        }
    }
}

impl IntoIterator for Body {
    type Item = Vec<u8>;

    type IntoIter = BodyChunkIterator;

    fn into_iter(mut self) -> Self::IntoIter {
        match self.0.take().unwrap() {
            BodyInner::Empty => BodyChunkIterator(None),
            BodyInner::Buffered(bytes) => {
                BodyChunkIterator(Some(BodyChunkIterInner::Single(bytes)))
            }
            BodyInner::Iter(chunks) => BodyChunkIterator(Some(BodyChunkIterInner::Iter(chunks))),
            BodyInner::Reader(reader, len) => {
                BodyChunkIterator(Some(BodyChunkIterInner::Reader(reader, len)))
            }
        }
    }
}

impl Drop for Body {
    fn drop(&mut self) {
        #[allow(unused_must_use)]
        match self.0.take() {
            Some(BodyInner::Reader(ref mut stream, Some(len))) => {
                let mut buf = vec![0_u8; len as usize];
                stream.read_exact(&mut buf);
            }
            Some(BodyInner::Reader(ref mut stream, None)) => {
                let mut buf = Vec::new();
                stream.read_to_end(&mut buf);
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

pub struct BodyReader(BodyReaderInner);

enum BodyReaderInner {
    Buffered(Cursor<Vec<u8>>),
    Iter(Box<dyn Iterator<Item = Vec<u8>>>, Option<Cursor<Vec<u8>>>),
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
                    *leftover = iter.next().map(Cursor::new);
                }
                Ok(0)
            }
        }
    }
}

pub struct BodyChunkIterator(Option<BodyChunkIterInner>);

enum BodyChunkIterInner {
    Single(Vec<u8>),
    Iter(Box<dyn Iterator<Item = Vec<u8>>>),
    Reader(Box<dyn Read>, Option<usize>),
}

impl Iterator for BodyChunkIterator {
    type Item = Vec<u8>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.0.take()? {
            BodyChunkIterInner::Single(bytes) => Some(bytes),
            BodyChunkIterInner::Iter(mut iter) => {
                let item = iter.next()?;
                self.0 = Some(BodyChunkIterInner::Iter(iter));
                Some(item)
            }
            BodyChunkIterInner::Reader(mut reader, Some(len)) => {
                let mut buf = vec![0_u8; len];
                reader.read_exact(&mut buf).ok()?;
                Some(buf)
            }
            BodyChunkIterInner::Reader(mut reader, None) => {
                let mut buf = vec![0_u8; 8 * 1024];
                match reader.read(&mut buf).ok()? {
                    0 => None,
                    bytes => {
                        self.0 = Some(BodyChunkIterInner::Reader(reader, None));
                        buf.resize(bytes, 0);
                        Some(buf)
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read};

    use crate::Body;

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
}
