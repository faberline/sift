//! A boxed future for ports, with no executor dependency.

use std::future::Future;
use std::pin::Pin;

pub(crate) type PortFuture<'a, T, E> = Pin<Box<dyn Future<Output = Result<T, E>> + Send + 'a>>;
