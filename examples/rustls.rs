// Run with: curl --insecure https://localhost:4444
#[cfg(feature = "rustls")]
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use std::{net::TcpListener, sync::Arc};

    use rustls::{crypto, ServerConfig, ServerConnection, StreamOwned};
    use rustls_pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
    use touche::{Response, Server, StatusCode};

    crypto::ring::default_provider().install_default().ok();

    let listener = TcpListener::bind("0.0.0.0:4444")?;

    let tls_cfg = {
        let certs = CertificateDer::pem_file_iter("examples/tls/cert.pem")?
            .filter_map(|cert| cert.ok())
            .collect();

        let key = PrivateKeyDer::from_pem_file("examples/tls/key.pem")?;

        let cfg = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)?;

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
        });

    Server::builder()
        .max_threads(100)
        .from_connections(connections)
        .serve(|_req| {
            Response::builder()
                .status(StatusCode::OK)
                .body("Hello from TLS")
        })?;

    Ok(())
}

#[cfg(not(feature = "rustls"))]
fn main() {
    println!("This example requires the rustls feature to be enabled");
}
