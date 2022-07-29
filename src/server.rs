//! HTTP Server
//!
//! The [`Server`] is responsible to read and parse a [`http::Request`], and then execute a [`Service`]
//! to generate a [`http::Response`].
//!
//! The implementation follows a simple thead per connection model, backed by a thread pool.
//!
//! # Example
//! ```no_run
//! use touche::{Response, Server, StatusCode};
//!
//! fn main() -> std::io::Result<()> {
//!     Server::builder()
//!         .max_threads(256)
//!         .bind("0.0.0.0:4444")
//!         .serve(|_req| {
//!             Response::builder()
//!                 .status(StatusCode::OK)
//!                 .body(())
//!         })
//! }
//! ```
use std::{
    error::Error,
    io,
    net::{TcpListener, ToSocketAddrs},
};

use threadpool::ThreadPool;

use crate::{serve, Connection, Service};

/// A listening HTTP server that accepts HTTP 1 connections.
pub struct Server<'a> {
    thread_pool: ThreadPool,
    incoming: Box<dyn Iterator<Item = Connection> + 'a>,
}

impl<'a> Server<'a> {
    /// Starts the [`ServerBuilder`].
    pub fn builder() -> ServerBuilder {
        Default::default()
    }

    /// Binds the [`Server`] to the given `addr`.
    ///
    /// # Panics
    ///
    /// This method will panic if binding to the address fails. For a non panic method to bind the
    /// server, see [`ServerBuilder::try_bind`].
    pub fn bind<A: ToSocketAddrs>(addr: A) -> Server<'static> {
        Self::builder().bind(addr)
    }

    /// Serves an [`Service`].
    ///
    /// # Example
    /// ```no_run
    /// # use touche::{Request, Response, Server, StatusCode};
    /// # fn main() -> std::io::Result<()> {
    /// Server::bind("0.0.0.0:4444").serve(|req: Request<_>| {
    ///     Response::builder()
    ///         .status(StatusCode::OK)
    ///         .body(req.into_body())
    /// })
    /// # }
    /// ```
    pub fn serve<S>(self, service: S) -> io::Result<()>
    where
        S: Service,
        S: Send + Clone + 'static,
    {
        for conn in self.incoming {
            let app = service.clone();
            self.thread_pool.execute(move || {
                serve(conn, app).ok();
            });
        }

        Ok(())
    }

    /// Serves an [`Connection`]. This should be used when you need to execute some logic on every
    /// connection.
    ///
    /// # Example
    /// ```no_run
    /// # use std::convert::Infallible;
    /// # use touche::{Connection, Response, Server, StatusCode};
    /// # fn main() -> std::io::Result<()> {
    /// Server::builder()
    ///     .bind("0.0.0.0:4444")
    ///     .make_service(|conn: &Connection| {
    ///         println!("New connection arrived: {:?}", conn.peer_addr());
    ///
    ///         Ok::<_, Infallible>(|_req| {
    ///             Response::builder()
    ///                 .status(StatusCode::OK)
    ///                 .body(())
    ///         })
    ///     })
    /// # }
    /// ```
    pub fn make_service<M>(self, make_service: M) -> io::Result<()>
    where
        M: MakeService,
        M: Send + Clone + 'static,
    {
        for conn in self.incoming {
            let app = make_service.clone();
            if let Ok(handler) = app.call(&conn) {
                self.thread_pool.execute(move || {
                    serve(conn, handler).ok();
                });
            }
        }

        Ok(())
    }
}

pub struct ServerBuilder {
    max_threads: usize,
}

impl Default for ServerBuilder {
    fn default() -> Self {
        Self { max_threads: 512 }
    }
}

impl ServerBuilder {
    /// Define the max number of threads this server may create. Defaults to `512`.
    ///
    /// # Example
    /// ```no_run
    /// # use touche::{Response, Server, StatusCode};
    /// # fn main() -> std::io::Result<()> {
    /// Server::builder()
    ///     .max_threads(12)
    ///     .bind("0.0.0.0:4444")
    ///     .serve(|_req| {
    ///         Response::builder()
    ///             .status(StatusCode::OK)
    ///             .body(())
    ///     })
    /// # }
    /// ```
    pub fn max_threads(self, max_threads: usize) -> Self {
        Self { max_threads }
    }

    /// Binds the [`Server`] to the given `addr`.
    ///
    /// # Panics
    ///
    /// This method will panic if binding to the address fails. For a non panic way to bind a
    /// server, see [`ServerBuilder::try_bind`].
    pub fn bind<A: ToSocketAddrs>(self, addr: A) -> Server<'static> {
        self.try_bind(addr).unwrap()
    }

    /// Tries to bind the server to the informed `addr`.
    pub fn try_bind<A: ToSocketAddrs>(self, addr: A) -> io::Result<Server<'static>> {
        let listener = TcpListener::bind(addr)?;
        Ok(self.from_connections(Box::new(TcpAcceptor { listener })))
    }

    /// Accepts connections from some [`Iterator`].
    ///
    /// # Example
    /// Running the server on a Unix socket
    /// ```no_run
    /// # use std::os::unix::net::UnixListener;
    /// # use touche::{Request, Response, Server, StatusCode};
    /// # fn main() -> std::io::Result<()> {
    /// let listener = UnixListener::bind("touche.socket")?;
    ///
    /// // Converting the Unix socket to a compatible [`Connection`]
    /// let connections = listener
    ///     .incoming()
    ///     .filter_map(|conn| conn.ok())
    ///     .map(|conn| conn.into());
    ///
    /// Server::builder()
    ///     .from_connections(connections)
    ///     .serve(|_req| {
    ///         Response::builder()
    ///             .status(StatusCode::OK)
    ///             .body("Hello from Unix socket!")
    ///     })
    /// # }
    /// ```
    pub fn from_connections<'a, T: IntoIterator<Item = Connection> + 'a>(
        self,
        conns: T,
    ) -> Server<'a> {
        Server {
            thread_pool: ThreadPool::new(self.max_threads),
            incoming: Box::new(conns.into_iter()),
        }
    }
}

struct TcpAcceptor {
    listener: TcpListener,
}

impl Iterator for TcpAcceptor {
    type Item = Connection;

    fn next(&mut self) -> Option<Self::Item> {
        Some(self.listener.accept().ok()?.into())
    }
}

pub trait MakeService {
    type Service: Service + Send;
    type Error: Into<Box<dyn Error + Send + Sync>>;

    fn call(&self, conn: &Connection) -> Result<Self::Service, Self::Error>;
}

impl<F, S, Err> MakeService for F
where
    F: Fn(&Connection) -> Result<S, Err>,
    F: Sync + Send + Clone,
    Err: Into<Box<dyn Error + Send + Sync>>,
    S: Service + Send,
{
    type Service = S;
    type Error = Err;

    fn call(&self, conn: &Connection) -> Result<Self::Service, Self::Error> {
        self(conn)
    }
}
