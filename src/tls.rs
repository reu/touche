use std::{
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream},
    sync::{Arc, Mutex},
    time::Duration,
};

use rustls::{ServerConnection, StreamOwned};

#[derive(Debug, Clone)]
pub struct RustlsConnection(Arc<Mutex<StreamOwned<ServerConnection, TcpStream>>>);

impl RustlsConnection {
    pub(crate) fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        let stream = self.0.lock().unwrap();
        stream.get_ref().set_read_timeout(timeout)?;
        Ok(())
    }

    pub(crate) fn into_inner(self) -> Result<StreamOwned<ServerConnection, TcpStream>, Self> {
        match Arc::try_unwrap(self.0) {
            Ok(conn) => Ok(conn.into_inner().unwrap()),
            Err(err) => Err(Self(err)),
        }
    }
}

impl From<StreamOwned<ServerConnection, TcpStream>> for RustlsConnection {
    fn from(tls: StreamOwned<ServerConnection, TcpStream>) -> Self {
        RustlsConnection(Arc::new(Mutex::new(tls)))
    }
}

impl RustlsConnection {
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.0
            .lock()
            .map_err(|_err| io::Error::new(io::ErrorKind::Other, "Failed to aquire lock"))?
            .sock
            .peer_addr()
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.0
            .lock()
            .map_err(|_err| io::Error::new(io::ErrorKind::Other, "Failed to aquire lock"))?
            .sock
            .local_addr()
    }
}

impl Read for RustlsConnection {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .map_err(|_err| io::Error::new(io::ErrorKind::Other, "Failed to aquire lock"))?
            .read(buf)
    }
}

impl Write for RustlsConnection {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .map_err(|_err| io::Error::new(io::ErrorKind::Other, "Failed to aquire lock"))?
            .write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0
            .lock()
            .map_err(|_err| io::Error::new(io::ErrorKind::Other, "Failed to aquire lock"))?
            .flush()
    }
}
