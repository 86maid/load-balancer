use async_trait::async_trait;
use tokio::task::yield_now;

use crate::{BoxLoadBalancer, LoadBalancer};
use std::{
    future::Future,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

/// A single entry in the interval load balancer.
/// Each entry contains:
/// - an interval (minimum duration before it can be reused),
/// - the last time it was used,
/// - and the associated value.
pub struct Entry<T>
where
    T: Send + Sync + Clone + 'static,
{
    pub interval: Duration,
    pub last: Option<RwLock<Instant>>,
    pub value: T,
}

impl<T> Clone for Entry<T>
where
    T: Send + Sync + Clone + 'static,
{
    fn clone(&self) -> Self {
        Self {
            interval: self.interval,
            last: self
                .last
                .as_ref()
                .and_then(|v| Some(RwLock::new(*v.try_read().unwrap()))),
            value: self.value.clone(),
        }
    }
}

/// A load balancer that allocates items based on a fixed interval.
/// Each entry can only be reused after its interval has elapsed since the last allocation.
#[derive(Clone)]
pub struct IntervalLoadBalancer<T>
where
    T: Send + Sync + Clone + 'static,
{
    inner: Arc<RwLock<Vec<Entry<T>>>>,
}

impl<T> IntervalLoadBalancer<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// Create a new `IntervalLoadBalancer` with a list of `(interval, value)` pairs.
    /// Each value will only be available after its interval has passed since the last allocation.
    pub fn new(entries: Vec<(Duration, T)>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(
                entries
                    .into_iter()
                    .map(|(interval, value)| Entry {
                        interval,
                        last: None,
                        value,
                    })
                    .collect(),
            )),
        }
    }

    /// Update the internal entries using an async callback.
    /// This allows dynamic reconfiguration of the load balancer.
    pub async fn update<F, R>(&self, handle: F) -> anyhow::Result<()>
    where
        F: Fn(Arc<RwLock<Vec<Entry<T>>>>) -> R,
        R: Future<Output = anyhow::Result<()>>,
    {
        handle(self.inner.clone()).await
    }
}

impl<T> LoadBalancer<T> for IntervalLoadBalancer<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// Allocate a value asynchronously.
    /// This will loop until a value becomes available, yielding in between attempts.
    fn alloc(&self) -> impl Future<Output = T> + Send {
        async move {
            loop {
                match LoadBalancer::try_alloc(self) {
                    Some(v) => return v,
                    None => yield_now().await,
                }
            }
        }
    }

    /// Try to allocate a value immediately without waiting.
    /// Returns `Some(value)` if an entry is available (interval elapsed),
    /// otherwise returns `None`.
    fn try_alloc(&self) -> Option<T> {
        let mut entries = self.inner.try_write().ok()?;

        for entry in entries.iter_mut() {
            if entry.last.is_none() {
                entry.last = Some(RwLock::new(Instant::now()));
                return Some(entry.value.clone());
            }

            let last = entry.last.as_ref().and_then(|lock| lock.try_read().ok())?;
            let now = Instant::now();

            if now.duration_since(*last) >= entry.interval {
                drop(last);
                entry.last = Some(RwLock::new(now));
                return Some(entry.value.clone());
            }
        }

        None
    }
}

#[async_trait]
impl<T> BoxLoadBalancer<T> for IntervalLoadBalancer<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// Asynchronous allocation (boxed trait version).
    async fn alloc(&self) -> T {
        LoadBalancer::alloc(self).await
    }

    /// Immediate allocation attempt (boxed trait version).
    fn try_alloc(&self) -> Option<T> {
        LoadBalancer::try_alloc(self)
    }
}
