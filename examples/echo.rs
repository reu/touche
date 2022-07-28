use touche::{Request, Response, Server, StatusCode};

fn main() -> std::io::Result<()> {
    Server::bind("0.0.0.0:4444").serve(|req: Request<_>| {
        Response::builder()
            .status(StatusCode::OK)
            .body(req.into_body())
    })
}
