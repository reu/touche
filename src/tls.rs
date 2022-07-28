use std::{
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream},
    sync::{Arc, Mutex},
};

use rustls::{ServerConnection, StreamOwned};

#[derive(Clone)]
pub struct RustlsConnection(Arc<Mutex<StreamOwned<ServerConnection, TcpStream>>>);

impl From<StreamOwned<ServerConnection, TcpStream>> for RustlsConnection {
    fn from(tls: StreamOwned<ServerConnection, TcpStream>) -> Self {
        RustlsConnection(Arc::new(Mutex::new(tls)))
    }
}

impl RustlsConnection {
    pub fn addr(&self) -> Option<SocketAddr> {
        self.0.lock().ok()?.sock.peer_addr().ok()
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
