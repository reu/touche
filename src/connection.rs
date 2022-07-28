use std::{
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream},
    os::unix::net::UnixStream,
};

pub struct Connection(ConnectionInner);

enum ConnectionInner {
    Tcp(TcpStream, SocketAddr),
    Unix(UnixStream),
}

impl Connection {
    pub fn addr(&self) -> Option<SocketAddr> {
        match self.0 {
            ConnectionInner::Tcp(_, addr) => Some(addr),
            ConnectionInner::Unix(_) => None,
        }
    }
}

impl Read for Connection {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Connection(ConnectionInner::Tcp(tcp, _)) => tcp.read(buf),
            Connection(ConnectionInner::Unix(unix)) => unix.read(buf),
        }
    }
}

impl Write for Connection {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Connection(ConnectionInner::Tcp(tcp, _)) => tcp.write(buf),
            Connection(ConnectionInner::Unix(unix)) => unix.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Connection(ConnectionInner::Tcp(tcp, _)) => tcp.flush(),
            Connection(ConnectionInner::Unix(unix)) => unix.flush(),
        }
    }
}

impl Clone for Connection {
    fn clone(&self) -> Self {
        match self {
            Connection(ConnectionInner::Tcp(tcp, addr)) => {
                Connection(ConnectionInner::Tcp(tcp.try_clone().unwrap(), *addr))
            }
            Connection(ConnectionInner::Unix(unix)) => {
                Connection(ConnectionInner::Unix(unix.try_clone().unwrap()))
            }
        }
    }
}

impl From<(TcpStream, SocketAddr)> for Connection {
    fn from((conn, addr): (TcpStream, SocketAddr)) -> Self {
        Connection(ConnectionInner::Tcp(conn, addr))
    }
}

impl From<UnixStream> for Connection {
    fn from(unix: UnixStream) -> Self {
        Connection(ConnectionInner::Unix(unix))
    }
}
