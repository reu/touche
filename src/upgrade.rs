use std::net::TcpStream;

pub trait UpgradeHandler: Sync + Send {
    fn handle(&self, stream: TcpStream);
}

impl<F: Fn(TcpStream) + Sync + Send> UpgradeHandler for F {
    fn handle(&self, stream: TcpStream) {
        self(stream)
    }
}

pub(crate) struct UpgradeExtension {
    pub(crate) handler: Box<dyn UpgradeHandler + 'static>,
}

pub trait Upgrade {
    fn upgrade(self, handle: impl UpgradeHandler + 'static) -> Self;
}

impl Upgrade for http::response::Builder {
    fn upgrade(self, handle: impl UpgradeHandler + 'static) -> Self {
        self.extension(UpgradeExtension {
            handler: Box::new(handle),
        })
    }
}

impl<T> Upgrade for http::Response<T> {
    fn upgrade(mut self, handle: impl UpgradeHandler + 'static) -> Self {
        self.extensions_mut().insert(UpgradeExtension {
            handler: Box::new(handle),
        });
        self
    }
}
