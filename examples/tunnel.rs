use std::{
    io,
    net::{Shutdown, TcpStream},
    thread,
};

use touche::{upgrade::Upgrade, Body, Connection, Method, Request, Response, Server, StatusCode};

// Try with: curl --proxy http://localhost:4444 https://en.wikipedia.org/wiki/HTTP_tunnel
fn main() -> io::Result<()> {
    Server::bind("0.0.0.0:4444").serve(|req: Request<_>| {
        if req.method() != Method::CONNECT {
            return Response::builder()
                .status(StatusCode::METHOD_NOT_ALLOWED)
                .header("allow", "connect")
                .body(Body::empty());
        }

        if let Some(address) = req.uri().authority().map(|a| a.to_string()) {
            Response::builder()
                .status(StatusCode::OK)
                .upgrade(move |conn: Connection| {
                    if let Ok(server) = TcpStream::connect(&address) {
                        match tunnel(conn.downcast().unwrap(), server) {
                            Ok((w, r)) => eprintln!("Tunneled bytes: {} (read) {} (write)", r, w),
                            Err(err) => eprintln!("Tunnel error: {}", err),
                        };
                    } else {
                        eprintln!("Could not connect to address: {}", address);
                    }
                })
                .body(Body::empty())
        } else {
            Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::from("Invalid address"))
        }
    })
}

fn tunnel(mut client: TcpStream, mut server: TcpStream) -> io::Result<(u64, u64)> {
    let mut client_writer = client.try_clone()?;
    let mut server_writer = server.try_clone()?;

    let client_to_server = thread::spawn(move || {
        let bytes = io::copy(&mut client, &mut server_writer)?;
        server_writer.shutdown(Shutdown::Both)?;
        io::Result::Ok(bytes)
    });

    let server_to_client = thread::spawn(move || {
        let bytes = io::copy(&mut server, &mut client_writer)?;
        client_writer.shutdown(Shutdown::Both)?;
        io::Result::Ok(bytes)
    });

    Ok((
        client_to_server.join().unwrap()?,
        server_to_client.join().unwrap()?,
    ))
}
