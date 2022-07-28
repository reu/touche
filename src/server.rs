use std::{
    error::Error,
    io,
    net::{TcpListener, ToSocketAddrs},
};

use threadpool::ThreadPool;

use crate::{connection::Connection, serve, App};

pub struct Server<'a> {
    thread_pool: ThreadPool,
    incoming: Box<dyn Iterator<Item = Connection> + 'a>,
}

impl<'a> Server<'a> {
    pub fn serve<Handle>(self, app: Handle) -> io::Result<()>
    where
        Handle: App,
        Handle: Send + Clone + 'static,
    {
        for conn in self.incoming {
            let app = app.clone();
            self.thread_pool.execute(move || {
                serve(conn, app).ok();
            });
        }

        Ok(())
    }

    pub fn serve_connection<Conn>(self, app: Conn) -> io::Result<()>
    where
        Conn: ConnectionHandler,
        Conn: Send + Clone + 'static,
    {
        for conn in self.incoming {
            let app = app.clone();
            if let Ok(handler) = app.handle_connection(&conn) {
                self.thread_pool.execute(move || {
                    serve(conn, handler).ok();
                });
            }
        }

        Ok(())
    }

    pub fn builder() -> ServerBuilder {
        Default::default()
    }

    pub fn bind<A: ToSocketAddrs>(addr: A) -> Server<'static> {
        Self::builder().bind(addr)
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
    pub fn max_threads(self, max_threads: usize) -> Self {
        Self { max_threads }
    }

    pub fn from_connections<'a, T: IntoIterator<Item = Connection> + 'a>(
        self,
        conns: T,
    ) -> Server<'a> {
        Server {
            thread_pool: ThreadPool::new(self.max_threads),
            incoming: Box::new(conns.into_iter()),
        }
    }

    pub fn bind<A: ToSocketAddrs>(self, addr: A) -> Server<'static> {
        self.try_bind(addr).unwrap()
    }

    pub fn try_bind<A: ToSocketAddrs>(self, addr: A) -> io::Result<Server<'static>> {
        let listener = TcpListener::bind(addr)?;
        Ok(self.from_connections(Box::new(TcpAcceptor { listener })))
    }
}

struct TcpAcceptor {
    listener: TcpListener,
}

impl Iterator for TcpAcceptor {
    type Item = Connection;

    fn next(&mut self) -> Option<Self::Item> {
        let (conn, _addr) = self.listener.accept().ok()?;
        Some(conn.into())
    }
}

pub trait ConnectionHandler {
    type App: App + Send;
    type Error: Into<Box<dyn Error + Send + Sync>>;

    fn handle_connection(&self, conn: &Connection) -> Result<Self::App, Self::Error>;
}

impl<F, A, Err> ConnectionHandler for F
where
    F: Fn(&Connection) -> Result<A, Err>,
    F: Sync + Send + Clone,
    Err: Into<Box<dyn Error + Send + Sync>>,
    A: App + Send,
{
    type App = A;
    type Error = Err;

    fn handle_connection(&self, conn: &Connection) -> Result<Self::App, Self::Error> {
        self(conn)
    }
}
