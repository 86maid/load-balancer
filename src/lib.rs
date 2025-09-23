//! # Load Balancer Library
//!
//! This library provides a set of generic load balancer implementations for distributing
//! workloads across multiple targets, such as clients, network endpoints, or resources.
//!
//! ## Modules
//!
//! - `ip` - Provides IP-based client load balancers with rate-limiting per IP.
//! - `limit` - Implements a limit-based load balancer that restricts allocations per entry
//!   within a configurable time interval.
//! - `random` - Randomly selects an entry from the available pool.
//! - `simple` - A simple sequential load balancer with optional Arc or Mutex wrappers.
//! - `threshold` - Implements threshold-based load balancing logic.
//!
//! The crate re-exports the [`get_if_addrs`] crate for retrieving network interface addresses.
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
//!     /// Returns `Some(T)` if successful, or `None` if no resource is available.
//!     fn alloc(&self) -> impl Future<Output = Option<T>> + Send;
//!
//!     /// Attempt to allocate a resource synchronously without awaiting.
//!     /// Returns `Some(T)` if successful, or `None` if no resource is available.
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
//!     /// Returns `Some(T)` if successful, or `None` if no resource is available.
//!     async fn alloc(&self) -> Option<T>;
//!
//!     /// Attempt to allocate a resource synchronously without awaiting.
//!     /// Returns `Some(T)` if successful, or `None` if no resource is available.
//!     fn try_alloc(&self) -> Option<T>;
//! }
//! ```
//!
//! ## Usage Example
//!
//! ```rust
//! use your_crate::random::RandomLoadBalancer;
//!
//! #[tokio::main]
//! async fn main() {
//!     let lb = RandomLoadBalancer::new(vec![1, 2, 3]);
//!     if let Some(value) = lb.alloc().await {
//!         println!("Allocated value: {}", value);
//!     }
//! }
//! ```
//!
//! ## Notes
//!
//! - All load balancers are `Send`, `Sync`, and `Clone`, making them suitable for multi-threaded
//!   asynchronous environments.
//! - The `alloc` method returns `Option<T>` rather than `Result`, reflecting that allocation
//!   failure is expected under normal conditions and not necessarily an error.
//! - `try_alloc` is non-blocking and will return `None` immediately if no resource is available.

pub mod ip;
pub mod limit;
pub mod random;
pub mod simple;
pub mod threshold;
pub use anyhow;
pub use get_if_addrs;

use async_trait::async_trait;
use std::future::Future;

pub trait LoadBalancer<T>: Send + Sync + Clone + 'static {
    fn alloc(&self) -> impl Future<Output = Option<T>> + Send;
    fn try_alloc(&self) -> Option<T>;
}

#[async_trait]
pub trait BoxLoadBalancer<T>: Send + Sync + Clone + 'static {
    async fn alloc(&self) -> Option<T>;
    fn try_alloc(&self) -> Option<T>;
}
