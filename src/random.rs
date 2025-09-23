use crate::{BoxLoadBalancer, LoadBalancer};
use async_trait::async_trait;
use rand::seq::IteratorRandom;
use std::future::Future;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A single entry in the RandomLoadBalancer.
#[derive(Clone)]
pub struct Entry<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// The underlying value of type `T`.
    pub value: T,
}

/// A load balancer that randomly selects an entry.
#[derive(Clone)]
pub struct RandomLoadBalancer<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// The inner list of entries.
    inner: Arc<RwLock<Vec<Entry<T>>>>,
}

impl<T> RandomLoadBalancer<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// Create a new random load balancer from a vector of values.
    pub fn new(inner: Vec<T>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(
                inner.into_iter().map(|value| Entry { value }).collect(),
            )),
        }
    }

    /// Update the inner entries using an async callback.
    pub async fn update<F, R>(&self, handle: F) -> anyhow::Result<()>
    where
        F: Fn(Arc<RwLock<Vec<Entry<T>>>>) -> R,
        R: Future<Output = anyhow::Result<()>>,
    {
        handle(self.inner.clone()).await
    }
}

impl<T> LoadBalancer<T> for RandomLoadBalancer<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// Asynchronously allocate a random entry.
    async fn alloc(&self) -> Option<T> {
        self.inner
            .read()
            .await
            .iter()
            .choose(&mut rand::rng())
            .map(|v| v.value.clone())
    }

    /// Try to allocate a random entry synchronously.
    fn try_alloc(&self) -> Option<T> {
        self.inner
            .try_read()
            .ok()?
            .iter()
            .choose(&mut rand::rng())
            .map(|v| v.value.clone())
    }
}

#[async_trait]
impl<T> BoxLoadBalancer<T> for RandomLoadBalancer<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// Asynchronously allocate a random entry.
    async fn alloc(&self) -> Option<T> {
        self.inner
            .read()
            .await
            .iter()
            .choose(&mut rand::rng())
            .map(|v| v.value.clone())
    }

    /// Try to allocate a random entry synchronously.
    fn try_alloc(&self) -> Option<T> {
        self.inner
            .try_read()
            .ok()?
            .iter()
            .choose(&mut rand::rng())
            .map(|v| v.value.clone())
    }
}
