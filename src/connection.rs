use std::{
    io::{self, Read, Write},
    net::TcpStream,
    os::unix::net::UnixStream,
};

pub struct Connection(ConnectionInner);

enum ConnectionInner {
    Tcp(TcpStream),
    Unix(UnixStream),
}

impl Read for Connection {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Connection(ConnectionInner::Tcp(tcp)) => tcp.read(buf),
            Connection(ConnectionInner::Unix(unix)) => unix.read(buf),
        }
    }
}

impl Write for Connection {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Connection(ConnectionInner::Tcp(tcp)) => tcp.write(buf),
            Connection(ConnectionInner::Unix(unix)) => unix.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Connection(ConnectionInner::Tcp(tcp)) => tcp.flush(),
            Connection(ConnectionInner::Unix(unix)) => unix.flush(),
        }
    }
}

impl Clone for Connection {
    fn clone(&self) -> Self {
        match self {
            Connection(ConnectionInner::Tcp(tcp)) => {
                Connection(ConnectionInner::Tcp(tcp.try_clone().unwrap()))
            }
            Connection(ConnectionInner::Unix(unix)) => {
                Connection(ConnectionInner::Unix(unix.try_clone().unwrap()))
            }
        }
    }
}

impl From<TcpStream> for Connection {
    fn from(tcp: TcpStream) -> Self {
        Connection(ConnectionInner::Tcp(tcp))
    }
}

impl From<UnixStream> for Connection {
    fn from(unix: UnixStream) -> Self {
        Connection(ConnectionInner::Unix(unix))
    }
}
