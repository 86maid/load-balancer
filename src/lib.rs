//! # Load Balancer Library
//!
//! This library provides a set of generic load balancer implementations for distributing
//! workloads across multiple targets, such as clients, network endpoints, or resources.
//!
//! ## Traits
//!
//! ### `LoadBalancer<T>`
//!
//! A generic trait for asynchronous or synchronous load balancing. Implementors provide
//! methods to allocate a resource from the pool.
//!
//! ```rust
//! use std::future::Future;
//!
//! pub trait LoadBalancer<T>: Send + Sync + Clone + 'static {
//!     /// Asynchronously allocate a resource.
//!     fn alloc(&self) -> impl Future<Output = T> + Send;
//!
//!     /// Attempt to allocate a resource synchronously without awaiting.
//!     fn try_alloc(&self) -> Option<T>;
//! }
//! ```
//!
//! ### `BoxLoadBalancer<T>`
//!
//! An async trait variant that can be used with `async_trait` for boxed trait objects
//! or dynamic dispatch. Provides similar functionality to `LoadBalancer` but supports
//! fully `async` method signatures.
//!
//! ```rust
//! use async_trait::async_trait;
//!
//! #[async_trait]
//! pub trait BoxLoadBalancer<T>: Send + Sync + Clone + 'static {
//!     /// Asynchronously allocate a resource.
//!     async fn alloc(&self) -> T;
//!
//!     /// Attempt to allocate a resource synchronously without awaiting.
//!     fn try_alloc(&self) -> Option<T>;
//! }
//! ```
pub mod general;
pub mod interval;
pub mod limit;
pub mod proxy;
pub mod random;
pub mod simple;
pub mod threshold;
pub use anyhow;
pub use get_if_addrs;
pub use reqwest;

use async_trait::async_trait;
use std::future::Future;

pub trait LoadBalancer<T>: Send + Sync + Clone + 'static {
    fn alloc(&self) -> impl Future<Output = T> + Send;
    fn try_alloc(&self) -> Option<T>;
}

#[async_trait]
pub trait BoxLoadBalancer<T>: Send + Sync + Clone + 'static {
    async fn alloc(&self) -> T;
    fn try_alloc(&self) -> Option<T>;
}
