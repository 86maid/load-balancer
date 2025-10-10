use crate::{BoxLoadBalancer, LoadBalancer};
use async_trait::async_trait;
use std::future::Future;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::RwLock;
use tokio::task::yield_now;

/// A single entry in the simple load balancer.
#[derive(Clone)]
pub struct Entry<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// The underlying value stored in this entry.
    pub value: T,
}

/// Internal reference structure for `SimpleLoadBalancer`.
pub struct SimpleLoadBalancerRef<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// The list of entries managed by the load balancer.
    pub entries: RwLock<Vec<Entry<T>>>,
    /// The current index for sequential allocation.
    pub cursor: AtomicUsize,
}

/// A simple load balancer that selects entries in sequential order.
#[derive(Clone)]
pub struct SimpleLoadBalancer<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// Shared inner state.
    inner: Arc<SimpleLoadBalancerRef<T>>,
}

impl<T> SimpleLoadBalancer<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// Create a new `SimpleLoadBalancer` from a list of values.
    pub fn new(entries: Vec<T>) -> Self {
        Self {
            inner: Arc::new(SimpleLoadBalancerRef {
                entries: RwLock::new(entries.into_iter().map(|v| Entry { value: v }).collect()),
                cursor: AtomicUsize::new(0),
            }),
        }
    }

    /// Update the inner state using an async callback.
    pub async fn update<F, R>(&self, handle: F) -> anyhow::Result<()>
    where
        F: Fn(Arc<SimpleLoadBalancerRef<T>>) -> R,
        R: Future<Output = anyhow::Result<()>>,
    {
        handle(self.inner.clone()).await
    }
}

impl<T> LoadBalancer<T> for SimpleLoadBalancer<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// Asynchronously allocate the next entry in sequence.
    async fn alloc(&self) -> T {
        loop {
            match LoadBalancer::try_alloc(self) {
                Some(v) => return v,
                None => yield_now().await,
            }
        }
    }

    /// Try to allocate the next entry in sequence without awaiting.
    fn try_alloc(&self) -> Option<T> {
        let entries = self.inner.entries.try_read().ok()?;

        if entries.is_empty() {
            return None;
        }

        Some(
            entries[self.inner.cursor.fetch_add(1, Ordering::Relaxed) % entries.len()]
                .value
                .clone(),
        )
    }
}

#[async_trait]
impl<T> BoxLoadBalancer<T> for SimpleLoadBalancer<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// Asynchronously allocate the next entry (BoxLoadBalancer version).
    async fn alloc(&self) -> T {
        LoadBalancer::alloc(self).await
    }

    /// Try to allocate the next entry (BoxLoadBalancer version).
    fn try_alloc(&self) -> Option<T> {
        LoadBalancer::try_alloc(self)
    }
}
