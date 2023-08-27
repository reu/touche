#![doc = include_str!("../README.md")]

pub mod body;
#[cfg(feature = "client")]
pub mod client;
mod connection;
mod read_queue;
mod request;
mod response;
#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "rustls")]
mod tls;
pub mod upgrade;

pub use body::Body;
pub use body::HttpBody;
#[cfg(feature = "client")]
pub use client::Client;
pub use connection::Connection;
#[doc(hidden)]
pub use http;
#[doc(no_inline)]
pub use http::HeaderMap;
pub use http::{header, Method, Request, Response, StatusCode, Uri, Version};
#[cfg(feature = "server")]
pub use server::Server;
