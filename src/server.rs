//! HTTP Server
//!
//! The [`Server`] is responsible to read and parse a [`http::Request`], and then execute a [`Service`]
//! to generate a [`http::Response`].
//!
//! The implementation follows a simple thread per connection model, backed by a thread pool.
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
    io::{self, BufReader, BufWriter, Write},
    net::{TcpListener, ToSocketAddrs},
    time::{Duration, SystemTime},
};

use headers::{HeaderMapExt, HeaderValue};
use http::{Method, Request, Response, StatusCode, Version};
#[cfg(feature = "threadpool")]
use threadpool::ThreadPool;

use crate::{
    body::HttpBody,
    read_queue::ReadQueue,
    request::{self, ParseError},
    response::{self, Outcome},
    Body, Connection,
};

type IncomingRequest = Request<Body>;

/// Maps [`Requests`](http::Request) to [`Responses`](http::Response).
///
/// Usually you don't need to manually implement this trait, as its `Fn` implementation might suffice
/// most of the needs.
///
/// ```no_run
/// # use std::convert::Infallible;
/// # use touche::{Body, Request, Response, Server, StatusCode};
/// fn app(req: Request<Body>) -> Result<Response<()>, Infallible> {
///     Ok(Response::builder().status(StatusCode::OK).body(()).unwrap())
/// }
///
/// fn main() -> std::io::Result<()> {
///     Server::bind("0.0.0.0:4444").serve(app)
/// }
/// ```
///
/// You might want to implement this trait if you wish to handle Expect 100-continue.
/// ```no_run
/// # use std::convert::Infallible;
/// # use headers::HeaderMapExt;
/// # use touche::{server::Service, Body, Request, Response, Server, StatusCode};
/// #[derive(Clone)]
/// struct UploadService {
///     max_length: u64,
/// }
///
/// impl Service for UploadService {
///     type Body = &'static str;
///     type Error = Infallible;
///
///     fn call(&mut self, _req: Request<Body>) -> Result<http::Response<Self::Body>, Self::Error> {
///         Ok(Response::builder()
///             .status(StatusCode::OK)
///             .body("Thanks for the info!")
///             .unwrap())
///     }
///
///     fn should_continue(&mut self, req: &Request<Body>) -> StatusCode {
///         match req.headers().typed_get::<headers::ContentLength>() {
///             Some(len) if len.0 <= self.max_length => StatusCode::CONTINUE,
///             _ => StatusCode::EXPECTATION_FAILED,
///         }
///     }
/// }
///
/// fn main() -> std::io::Result<()> {
///     Server::bind("0.0.0.0:4444").serve(UploadService { max_length: 1024 })
/// }
/// ```
pub trait Service {
    type Body: HttpBody;
    type Error: Into<Box<dyn Error + Send + Sync>>;

    fn call(&mut self, request: IncomingRequest) -> Result<Response<Self::Body>, Self::Error>;

    fn should_continue(&mut self, _: &IncomingRequest) -> StatusCode {
        StatusCode::CONTINUE
    }
}

impl<F, Body, Err> Service for F
where
    F: FnMut(IncomingRequest) -> Result<Response<Body>, Err>,
    Body: HttpBody,
    Err: Into<Box<dyn Error + Send + Sync>>,
{
    type Body = Body;
    type Error = Err;

    fn call(&mut self, request: IncomingRequest) -> Result<Response<Self::Body>, Self::Error> {
        self(request)
    }
}

/// A listening HTTP server that accepts HTTP 1 connections.
pub struct Server<'a> {
    #[cfg(feature = "threadpool")]
    thread_pool: ThreadPool,
    incoming: Box<dyn Iterator<Item = Connection> + 'a>,
}

impl From<TcpListener> for Server<'static> {
    fn from(listener: TcpListener) -> Self {
        Self::builder().from_connections(TcpAcceptor { listener })
    }
}

impl Server<'_> {
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

    /// Serves an [`Service`] on a thread per connection model, backed by a thread pool.
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
    #[cfg(feature = "threadpool")]
    pub fn serve<S>(self, service: S) -> io::Result<()>
    where
        S: Service,
        S: Send + Clone + 'static,
    {
        for conn in self.incoming {
            let mut app = service.clone();
            self.thread_pool.execute(move || {
                serve(conn, &mut app).ok();
            });
        }

        Ok(())
    }

    /// Serves an [`Service`] on a single thread. This is useful when your [`Service`] is not
    /// [`Send`]. Note that if a connection is kept alive on this mode, no other request may be
    /// served before the said connection is closed.
    ///
    /// # Example
    /// ```no_run
    /// # use touche::{Request, Response, Server, StatusCode};
    /// # fn main() -> std::io::Result<()> {
    /// Server::bind("0.0.0.0:4444").serve_single_thread(|req: Request<_>| {
    ///     Response::builder()
    ///         .status(StatusCode::OK)
    ///         .body(req.into_body())
    /// })
    /// # }
    /// ```
    pub fn serve_single_thread<S>(self, mut service: S) -> io::Result<()>
    where
        S: Service,
    {
        for conn in self.incoming {
            serve(conn, &mut service).ok();
        }
        Ok(())
    }

    /// Hook into how a [`Connection`] handles its requests. This should be used when you need to
    /// execute some logic on every connection.
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
    ///
    /// # Per connection shared mutable state
    ///
    /// You share any state for a given Connection without having to worry about any
    /// synchronization on it.
    /// ```no_run
    /// # use std::convert::Infallible;
    /// # use touche::{Connection, Response, Server, StatusCode};
    /// # fn main() -> std::io::Result<()> {
    /// Server::builder()
    ///     .bind("0.0.0.0:4444")
    ///     .make_service(move |_conn: &Connection| {
    ///         let mut counter = 0;
    ///
    ///         Ok::<_, Infallible>(move |_req| {
    ///             counter += 1;
    ///
    ///             Response::builder()
    ///                 .status(StatusCode::OK)
    ///                 .body(format!("Requests on this connection: {counter}"))
    ///         })
    ///     })
    /// # }
    /// ```
    #[cfg(feature = "threadpool")]
    pub fn make_service<M>(self, make_service: M) -> io::Result<()>
    where
        M: MakeService + 'static,
        <M as MakeService>::Service: Send,
    {
        for conn in self.incoming {
            if let Ok(mut handler) = make_service.call(&conn) {
                self.thread_pool.execute(move || {
                    serve(conn, &mut handler).ok();
                });
            }
        }

        Ok(())
    }
}

pub struct ServerBuilder {
    #[cfg(feature = "threadpool")]
    max_threads: usize,
    read_timeout: Option<Duration>,
}

impl Default for ServerBuilder {
    fn default() -> Self {
        Self {
            #[cfg(feature = "threadpool")]
            max_threads: 512,
            read_timeout: None,
        }
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
    #[cfg(feature = "threadpool")]
    pub fn max_threads(self, max_threads: usize) -> Self {
        Self {
            max_threads,
            ..self
        }
    }

    /// Sets the time limit that connections will be kept alive when no data is received.
    /// Defaults to no time limit at all.
    ///
    /// # Example
    /// ```no_run
    /// # use std::time::Duration;
    /// # use touche::{Response, Server, StatusCode};
    /// # fn main() -> std::io::Result<()> {
    /// Server::builder()
    ///     // Close the connection if no data arrives in 10 seconds
    ///     .read_timeout(Duration::from_secs(10))
    ///     .bind("0.0.0.0:4444")
    ///     .serve(|_req| {
    ///         Response::builder()
    ///             .status(StatusCode::OK)
    ///             .body(())
    ///     })
    /// # }
    /// ```
    ///
    /// # Example with upgraded connection
    ///
    /// Be careful when using this option with upgraded connections, as the underlying protocol may
    /// need some different timeout configurations. In that case, you can use the
    /// [`Connection::set_read_timeout`] to set per connection configuration.
    ///
    /// ```no_run
    /// # use std::{
    /// #     io::{Read, Write},
    /// #     time::Duration,
    /// # };
    /// # use touche::{header, upgrade::Upgrade, Connection, Response, Server, StatusCode};
    /// # fn main() -> std::io::Result<()> {
    /// Server::builder()
    ///     // Sets the server read timeout to 10 seconds
    ///     .read_timeout(Duration::from_secs(10))
    ///     .bind("0.0.0.0:4444")
    ///     .serve(|_req| {
    ///         Response::builder()
    ///             .status(StatusCode::SWITCHING_PROTOCOLS)
    ///             .header(header::UPGRADE, "echo")
    ///             .upgrade(|mut conn: Connection| {
    ///                 // Don't timeout on the upgraded connection
    ///                 conn.set_read_timeout(None).unwrap();
    ///
    ///                 loop {
    ///                     let mut buf = [0; 1024];
    ///                     match conn.read(&mut buf) {
    ///                         Ok(n) if n > 0 => conn.write(&buf[0..n]).unwrap(),
    ///                         _ => break,
    ///                     };
    ///                 }
    ///             })
    ///             .body(())
    ///     })
    /// # }
    /// ```
    pub fn read_timeout<T: Into<Option<Duration>>>(self, timeout: T) -> Self {
        Self {
            read_timeout: timeout.into(),
            ..self
        }
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
        Ok(self.from_connections(TcpAcceptor { listener }))
    }

    /// Accepts connections from some [`Iterator`].
    pub fn from_connections<'a, T: IntoIterator<Item = Connection> + 'a>(
        self,
        conns: T,
    ) -> Server<'a> {
        Server {
            #[cfg(feature = "threadpool")]
            thread_pool: ThreadPool::new(self.max_threads),
            incoming: Box::new(conns.into_iter().filter_map(move |conn| {
                conn.set_read_timeout(self.read_timeout).ok()?;
                Some(conn)
            })),
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
    type Service: Service;
    type Error: Into<Box<dyn Error + Send + Sync>>;

    fn call(&self, conn: &Connection) -> Result<Self::Service, Self::Error>;
}

impl<F, S, Err> MakeService for F
where
    F: Fn(&Connection) -> Result<S, Err>,
    Err: Into<Box<dyn Error + Send + Sync>>,
    S: Service + Send,
{
    type Service = S;
    type Error = Err;

    fn call(&self, conn: &Connection) -> Result<Self::Service, Self::Error> {
        self(conn)
    }
}

fn serve<C: Into<Connection>, A: Service>(stream: C, app: &mut A) -> io::Result<()> {
    let conn = stream.into();
    let mut read_queue = ReadQueue::new(BufReader::new(conn.clone()));

    let mut reader = read_queue.enqueue();
    let mut writer = BufWriter::new(conn);

    loop {
        match request::parse_request(reader) {
            Ok(req) => {
                reader = read_queue.enqueue();

                let asks_for_close = req
                    .headers()
                    .typed_get::<headers::Connection>()
                    .filter(|conn| conn.contains("close"))
                    .is_some();

                let asks_for_keep_alive = req
                    .headers()
                    .typed_get::<headers::Connection>()
                    .filter(|conn| conn.contains("keep-alive"))
                    .is_some();

                let version = req.version();
                let method = req.method().clone();

                let demands_close = match version {
                    Version::HTTP_09 => true,
                    Version::HTTP_10 => !asks_for_keep_alive,
                    _ => asks_for_close,
                };

                let expects_continue = req
                    .headers()
                    .typed_get::<headers::Expect>()
                    .filter(|expect| expect == &headers::Expect::CONTINUE)
                    .is_some();

                if expects_continue {
                    match app.should_continue(&req) {
                        status @ StatusCode::CONTINUE => {
                            let res = Response::builder().status(status).body(()).unwrap();
                            response::write_response(res, &mut writer, true)?;
                            writer.flush()?;
                        }
                        status => {
                            let res = Response::builder().status(status).body(()).unwrap();
                            response::write_response(res, &mut writer, true)?;
                            writer.flush()?;
                            continue;
                        }
                    };
                }

                let mut res = app
                    .call(req)
                    .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

                *res.version_mut() = version;

                if version == Version::HTTP_10 && !asks_for_keep_alive {
                    res.headers_mut()
                        .insert("connection", HeaderValue::from_static("close"));
                }

                if res.headers().typed_get::<headers::Date>().is_none() {
                    res.headers_mut()
                        .typed_insert(headers::Date::from(SystemTime::now()));
                }

                let should_write_body = match method {
                    Method::HEAD => false,
                    Method::CONNECT => res.status().is_success(),
                    _ => true,
                };

                match response::write_response(res, &mut writer, should_write_body)? {
                    Outcome::KeepAlive if demands_close => break,
                    Outcome::KeepAlive => writer.flush()?,
                    Outcome::Close => break,
                    Outcome::Upgrade(upgrade) => {
                        drop(reader);
                        drop(read_queue);
                        upgrade.handler.handle(writer.into_inner()?);
                        break;
                    }
                }
            }
            Err(ParseError::ConnectionClosed) => break,
            Err(err) => return Err(io::Error::new(io::ErrorKind::Other, err)),
        }
    }

    Ok(())
}
