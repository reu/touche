use std::{
    fs::File,
    io::{self, Read},
};

#[derive(Default)]
pub enum Body {
    #[default]
    Empty,
    Buffered(Vec<u8>),
    Chunked(Box<dyn Iterator<Item = Vec<u8>>>),
    Reader(Box<dyn Read>, Option<usize>),
}

impl Body {
    pub fn empty() -> Self {
        Body::Empty
    }

    pub fn chunked<T: Into<Vec<u8>>>(chunks: impl IntoIterator<Item = T> + 'static) -> Self {
        Body::Chunked(Box::new(chunks.into_iter().map(|chunk| chunk.into())))
    }

    pub fn from_reader<T: Into<Option<usize>>>(reader: impl Read + 'static, length: T) -> Self {
        Body::Reader(Box::new(reader), length.into())
    }

    pub fn into_bytes(self) -> io::Result<Vec<u8>> {
        match self {
            Body::Empty => Ok(Vec::new()),
            Body::Buffered(bytes) => Ok(bytes),
            Body::Chunked(chunks) => Ok(chunks.flatten().collect()),
            Body::Reader(mut stream, Some(len)) => {
                let mut buf = vec![0_u8; len];
                stream.read_exact(&mut buf)?;
                Ok(buf)
            }
            Body::Reader(mut stream, None) => {
                let mut buf = Vec::new();
                stream.read_to_end(&mut buf)?;
                Ok(buf)
            }
        }
    }
}

impl From<Vec<u8>> for Body {
    fn from(body: Vec<u8>) -> Self {
        Body::Buffered(body)
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
    type Error = &'static str;

    fn try_from(file: File) -> Result<Self, Self::Error> {
        match file.metadata() {
            Ok(meta) if meta.is_file() => Ok(Body::from_reader(file, meta.len() as usize)),
            _ => Err("Invalid file"),
        }
    }
}