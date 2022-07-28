use std::io::{BufRead, BufReader, BufWriter, Write};

use http::{header, Response, StatusCode};
use touche::{connection::Connection, upgrade::Upgrade, Server};

fn main() -> std::io::Result<()> {
    Server::bind("0.0.0.0:4444").serve(|_req| {
        Response::builder()
            .status(StatusCode::SWITCHING_PROTOCOLS)
            .header(header::UPGRADE, "line-protocol")
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
}
