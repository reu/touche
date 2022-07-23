use std::{
    io::{self, Cursor, Read},
    iter,
};

pub trait HttpBody: Sized {
    type BodyReader: Read;
    type Chunks: Iterator<Item = Vec<u8>>;

    fn len(&self) -> Option<u64>;

    fn is_empty(&self) -> bool {
        matches!(self.len(), Some(0))
    }

    fn into_reader(self) -> Self::BodyReader;

    fn into_chunks(self) -> Self::Chunks;

    fn into_bytes(self) -> io::Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(1024);
        self.into_reader().read_to_end(&mut buf)?;
        Ok(buf)
    }
}

impl HttpBody for () {
    type BodyReader = io::Empty;
    type Chunks = iter::Empty<Vec<u8>>;

    fn len(&self) -> Option<u64> {
        Some(0)
    }

    fn into_reader(self) -> Self::BodyReader {
        io::empty()
    }

    fn into_bytes(self) -> io::Result<Vec<u8>> {
        Ok(Vec::new())
    }

    fn into_chunks(self) -> Self::Chunks {
        iter::empty()
    }
}

impl HttpBody for String {
    type BodyReader = Cursor<Vec<u8>>;
    type Chunks = iter::Once<Vec<u8>>;

    fn len(&self) -> Option<u64> {
        self.len().try_into().ok()
    }

    fn into_reader(self) -> Self::BodyReader {
        Cursor::new(self.into_bytes())
    }

    fn into_bytes(self) -> io::Result<Vec<u8>> {
        Ok(self.into_bytes())
    }

    fn into_chunks(self) -> Self::Chunks {
        iter::once(self.into_bytes())
    }
}

impl HttpBody for &str {
    type BodyReader = Cursor<Vec<u8>>;
    type Chunks = iter::Once<Vec<u8>>;

    fn len(&self) -> Option<u64> {
        str::len(self).try_into().ok()
    }

    fn into_reader(self) -> Self::BodyReader {
        Cursor::new(self.bytes().collect())
    }

    fn into_bytes(self) -> io::Result<Vec<u8>> {
        Ok(self.bytes().collect())
    }

    fn into_chunks(self) -> Self::Chunks {
        iter::once(self.bytes().collect())
    }
}

impl HttpBody for &'static [u8] {
    type BodyReader = &'static [u8];
    type Chunks = iter::Once<Vec<u8>>;

    fn len(&self) -> Option<u64> {
        (*self).len().try_into().ok()
    }

    fn into_reader(self) -> Self::BodyReader {
        self
    }

    fn into_bytes(self) -> io::Result<Vec<u8>> {
        Ok(self.to_vec())
    }

    fn into_chunks(self) -> Self::Chunks {
        iter::once(self.to_vec())
    }
}

impl HttpBody for Vec<u8> {
    type BodyReader = Cursor<Vec<u8>>;
    type Chunks = iter::Once<Vec<u8>>;

    fn len(&self) -> Option<u64> {
        self.len().try_into().ok()
    }

    fn into_reader(self) -> Self::BodyReader {
        Cursor::new(self)
    }

    fn into_bytes(self) -> io::Result<Vec<u8>> {
        Ok(self)
    }

    fn into_chunks(self) -> Self::Chunks {
        iter::once(self)
    }
}
