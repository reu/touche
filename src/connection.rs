use std::{
    any::{Any, TypeId},
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream},
    time::Duration,
};

#[cfg(feature = "unix-sockets")]
use std::os::unix::net::UnixStream;

#[cfg(feature = "rustls")]
use crate::tls::RustlsConnection;

/// Abstracts away the several types of streams where HTTP can be deployed.
#[derive(Debug)]
pub struct Connection(ConnectionInner);

#[derive(Debug)]
enum ConnectionInner {
    Tcp(TcpStream),
    #[cfg(feature = "unix-sockets")]
    Unix(UnixStream),
    #[cfg(feature = "rustls")]
    Rustls(RustlsConnection),
}

impl Connection {
    pub fn peer_addr(&self) -> Option<SocketAddr> {
        match self.0 {
            ConnectionInner::Tcp(ref tcp) => tcp.peer_addr().ok(),
            #[cfg(feature = "unix-sockets")]
            ConnectionInner::Unix(_) => None,
            #[cfg(feature = "rustls")]
            ConnectionInner::Rustls(ref tls) => tls.peer_addr().ok(),
        }
    }

    pub fn local_addr(&self) -> Option<SocketAddr> {
        match self.0 {
            ConnectionInner::Tcp(ref tcp) => tcp.local_addr().ok(),
            #[cfg(feature = "unix-sockets")]
            ConnectionInner::Unix(_) => None,
            #[cfg(feature = "rustls")]
            ConnectionInner::Rustls(ref tls) => tls.local_addr().ok(),
        }
    }

    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> Result<(), io::Error> {
        match self.0 {
            ConnectionInner::Tcp(ref tcp) => tcp.set_read_timeout(timeout),
            #[cfg(feature = "unix-sockets")]
            ConnectionInner::Unix(ref unix) => unix.set_read_timeout(timeout),
            #[cfg(feature = "rustls")]
            ConnectionInner::Rustls(ref tls) => tls.set_read_timeout(timeout),
        }
    }

    pub fn set_nodelay(&self, nodelay: bool) -> Result<(), io::Error> {
        match self.0 {
            ConnectionInner::Tcp(ref tcp) => tcp.set_nodelay(nodelay),
            #[cfg(feature = "unix-sockets")]
            ConnectionInner::Unix(_) => Ok(()),
            #[cfg(feature = "rustls")]
            ConnectionInner::Rustls(ref tls) => tls.set_nodelay(nodelay),
        }
    }

    /// Attempts to downcast the [`Connection`] into the underlying stream.
    /// On error returns the [`Connection`] back.
    ///
    /// # Example
    /// ```no_run
    /// # use std::net::{TcpListener, TcpStream};
    /// # use touche::Connection;
    /// # fn main() -> std::io::Result<()> {
    /// # let listener = TcpListener::bind("0.0.0.0:4444")?;
    /// # let connection = Connection::from(listener.accept()?);
    /// if let Ok(tcp) = connection.downcast::<TcpStream>() {
    ///     println!("Connection is a TcpStream");
    /// } else {
    ///     println!("Connection is not a TcpStream");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn downcast<T: Any>(self) -> Result<T, Self> {
        match self.0 {
            ConnectionInner::Tcp(tcp) if Any::type_id(&tcp) == TypeId::of::<T>() => {
                let tcp = Box::new(tcp) as Box<dyn Any>;
                Ok(tcp.downcast().map(|tcp| *tcp).unwrap())
            }

            #[cfg(feature = "unix-sockets")]
            ConnectionInner::Unix(unix) if Any::type_id(&unix) == TypeId::of::<T>() => {
                let unix = Box::new(unix) as Box<dyn Any>;
                Ok(unix.downcast().map(|unix| *unix).unwrap())
            }

            #[cfg(feature = "rustls")]
            ConnectionInner::Rustls(tls) => match tls.into_inner() {
                Ok(tls) if Any::type_id(&tls) == TypeId::of::<T>() => {
                    let tls = Box::new(tls) as Box<dyn Any>;
                    Ok(tls.downcast().map(|tls| *tls).unwrap())
                }
                Ok(tls) => Err(Self(ConnectionInner::Rustls(tls.into()))),
                Err(tls) => Err(Self(ConnectionInner::Rustls(tls))),
            },

            conn => Err(Self(conn)),
        }
    }
}

impl Read for Connection {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Connection(ConnectionInner::Tcp(tcp)) => tcp.read(buf),
            #[cfg(feature = "unix-sockets")]
            Connection(ConnectionInner::Unix(unix)) => unix.read(buf),
            #[cfg(feature = "rustls")]
            Connection(ConnectionInner::Rustls(tls)) => tls.read(buf),
        }
    }
}

impl Write for Connection {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Connection(ConnectionInner::Tcp(tcp)) => tcp.write(buf),
            #[cfg(feature = "unix-sockets")]
            Connection(ConnectionInner::Unix(unix)) => unix.write(buf),
            #[cfg(feature = "rustls")]
            Connection(ConnectionInner::Rustls(tls)) => tls.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Connection(ConnectionInner::Tcp(tcp)) => tcp.flush(),
            #[cfg(feature = "unix-sockets")]
            Connection(ConnectionInner::Unix(unix)) => unix.flush(),
            #[cfg(feature = "rustls")]
            Connection(ConnectionInner::Rustls(tls)) => tls.flush(),
        }
    }
}

impl Clone for Connection {
    fn clone(&self) -> Self {
        match self {
            Connection(ConnectionInner::Tcp(tcp)) => {
                Connection(ConnectionInner::Tcp(tcp.try_clone().unwrap()))
            }
            #[cfg(feature = "unix-sockets")]
            Connection(ConnectionInner::Unix(unix)) => {
                Connection(ConnectionInner::Unix(unix.try_clone().unwrap()))
            }
            #[cfg(feature = "rustls")]
            Connection(ConnectionInner::Rustls(tls)) => {
                Connection(ConnectionInner::Rustls(tls.clone()))
            }
        }
    }
}

impl From<TcpStream> for Connection {
    fn from(conn: TcpStream) -> Self {
        Connection(ConnectionInner::Tcp(conn))
    }
}

impl From<(TcpStream, SocketAddr)> for Connection {
    fn from((conn, _addr): (TcpStream, SocketAddr)) -> Self {
        Connection(ConnectionInner::Tcp(conn))
    }
}

#[cfg(feature = "unix-sockets")]
impl From<UnixStream> for Connection {
    fn from(unix: UnixStream) -> Self {
        Connection(ConnectionInner::Unix(unix))
    }
}

#[cfg(feature = "rustls")]
impl From<rustls::StreamOwned<rustls::ServerConnection, TcpStream>> for Connection {
    fn from(tls: rustls::StreamOwned<rustls::ServerConnection, TcpStream>) -> Self {
        Connection(ConnectionInner::Rustls(tls.into()))
    }
}
