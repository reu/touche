use std::{
    io::{BufRead, BufReader, BufWriter, Write},
    net::TcpListener,
    thread,
};

use http::{header::UPGRADE, Response, StatusCode};
use shrike::{connection::Connection, upgrade::Upgrade};

fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:4444")?;

    for stream in listener.incoming() {
        let stream = stream?;
        thread::spawn(move || {
            shrike::serve(stream, |_req| {
                Response::builder()
                    .status(StatusCode::SWITCHING_PROTOCOLS)
                    .header(UPGRADE, "line-protocol")
                    .upgrade(|stream: Connection| {
                        let reader = BufReader::new(stream.clone());
                        let mut writer = BufWriter::new(stream);

                        // Just a simple protocol that will echo every line sent
                        for line in reader.lines() {
                            match line {
                                Ok(line) if line.as_str() == "quit" => break,
                                Ok(line) => {
                                    writer.write_all(format!("{line}\n").as_bytes()).unwrap();
                                    writer.flush().unwrap();
                                }
                                Err(_err) => break,
                            };
                        }
                    })
                    .body("Upgrading...\n")
            })
        });
    }

    Ok(())
}
