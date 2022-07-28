#[cfg(not(feature = "rustls"))]
fn main() {
    println!("This example requires the rustls feature to be enabled");
}

#[cfg(feature = "rustls")]
fn main() -> std::io::Result<()> {
    use std::{
        io::{self, ErrorKind::Other},
        net::TcpListener,
        sync::Arc,
    };

    use rustls::{ServerConfig, ServerConnection, StreamOwned};
    use touche::{Response, Server, StatusCode};

    let listener = TcpListener::bind("0.0.0.0:4444")?;

    let tls_cfg = {
        let certs = certs::load_certs("examples/tls/cert.pem")?;
        let key = certs::load_private_key("examples/tls/key.pem")?;

        let cfg = ServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| io::Error::new(Other, e))?;

        Arc::new(cfg)
    };

    let connections = listener
        .incoming()
        .filter_map(|conn| conn.ok())
        .filter_map(|conn| {
            Some(StreamOwned::new(
                ServerConnection::new(tls_cfg.clone()).ok()?,
                conn,
            ))
        })
        .map(|tls_conn| tls_conn.into());

    Server::builder()
        .max_threads(100)
        .from_connections(connections)
        .serve(|_req| {
            Response::builder()
                .status(StatusCode::OK)
                .body("Hello from TLS")
        })
}

#[cfg(feature = "rustls")]
mod certs {
    use std::{
        fs,
        io::{self, ErrorKind::Other},
    };

    use rustls::{Certificate, PrivateKey};
    use rustls_pemfile as pem;

    pub fn load_certs(filename: &str) -> io::Result<Vec<Certificate>> {
        let certfile = fs::File::open(filename)?;
        let mut reader = io::BufReader::new(certfile);

        let certs = pem::certs(&mut reader).map_err(|err| io::Error::new(Other, err))?;

        Ok(certs.into_iter().map(Certificate).collect())
    }

    pub fn load_private_key(filename: &str) -> io::Result<PrivateKey> {
        let keyfile = fs::File::open(filename)?;
        let mut reader = io::BufReader::new(keyfile);

        let keys = pem::rsa_private_keys(&mut reader).map_err(|err| io::Error::new(Other, err))?;

        Ok(PrivateKey(keys[0].clone()))
    }
}
