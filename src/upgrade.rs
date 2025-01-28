use std::sync::Arc;

use thiserror::Error;

use crate::connection::Connection;

pub trait UpgradeHandler: Sync + Send {
    fn handle(&self, stream: Connection);
}

impl<F: Fn(Connection) + Sync + Send> UpgradeHandler for F {
    fn handle(&self, stream: Connection) {
        self(stream)
    }
}

#[derive(Clone)]
pub(crate) struct UpgradeExtension {
    pub(crate) handler: Arc<dyn UpgradeHandler + 'static>,
}

pub trait Upgrade {
    fn upgrade(self, handle: impl UpgradeHandler + 'static) -> Self;
}

impl Upgrade for http::response::Builder {
    fn upgrade(self, handle: impl UpgradeHandler + 'static) -> Self {
        self.extension(UpgradeExtension {
            handler: Arc::new(handle),
        })
    }
}

impl<T> Upgrade for http::Response<T> {
    fn upgrade(mut self, handle: impl UpgradeHandler + 'static) -> Self {
        self.extensions_mut().insert(UpgradeExtension {
            handler: Arc::new(handle),
        });
        self
    }
}

#[derive(Debug, Error)]
pub enum ClientUpgradeError {
    #[error("connection not upgradable")]
    ConnectionNotUpgradable,
}

pub trait ClientUpgrade {
    fn into_upgraded(self) -> Result<Connection, ClientUpgradeError>;
}

impl<T> ClientUpgrade for http::Response<T> {
    fn into_upgraded(mut self) -> Result<Connection, ClientUpgradeError> {
        self.extensions_mut()
            .remove()
            .ok_or(ClientUpgradeError::ConnectionNotUpgradable)
    }
}
