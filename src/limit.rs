use crate::{BoxLoadBalancer, LoadBalancer};
use async_trait::async_trait;
use std::sync::atomic::Ordering::Acquire;
use std::sync::atomic::Ordering::Release;
use std::{
    future::Future,
    sync::{Arc, atomic::AtomicU64},
    time::Duration,
};
use tokio::{
    spawn,
    sync::{Mutex, RwLock},
    task::{JoinHandle, yield_now},
    time::sleep,
};

/// A single entry in the `LimitLoadBalancer`.
///
/// Tracks the maximum number of allowed allocations within the interval
/// and the current allocation count.
pub struct Entry<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// Maximum number of allocations allowed for this entry in the interval.
    pub max_count: u64,
    /// Current allocation count within the interval.
    pub count: AtomicU64,
    /// The underlying value/resource of type `T`.
    pub value: T,
}

impl<T> Clone for Entry<T>
where
    T: Send + Sync + Clone + 'static,
{
    fn clone(&self) -> Self {
        Self {
            max_count: self.max_count.clone(),
            count: self.count.load(Acquire).into(),
            value: self.value.clone(),
        }
    }
}

/// Internal reference structure for `LimitLoadBalancer`.
///
/// Holds the entries and the interval timer.
pub struct LimitLoadBalancerRef<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// The entries managed by this load balancer.
    pub entries: RwLock<Vec<Entry<T>>>,
    /// Timer task handle for resetting counts periodically.
    pub timer: Mutex<Option<JoinHandle<()>>>,
    /// The interval at which counts are reset.
    pub interval: RwLock<Duration>,
}

/// A load balancer that limits the number of allocations per entry
/// over a specified time interval.
///
/// This implementation supports both async and sync allocation.
#[derive(Clone)]
pub struct LimitLoadBalancer<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// Shared reference to the internal state.
    inner: Arc<LimitLoadBalancerRef<T>>,
}

impl<T> LimitLoadBalancer<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// Create a new `LimitLoadBalancer` with the default interval of 1 second.
    ///
    /// # Arguments
    ///
    /// * `entries` - A vector of tuples `(max_count, value)`.
    pub fn new(entries: Vec<(u64, T)>) -> Self {
        Self {
            inner: Arc::new(LimitLoadBalancerRef {
                entries: entries
                    .into_iter()
                    .map(|(max_count, value)| Entry {
                        max_count,
                        value,
                        count: 0.into(),
                    })
                    .collect::<Vec<_>>()
                    .into(),
                timer: None.into(),
                interval: Duration::from_secs(1).into(),
            }),
        }
    }

    /// Create a new `LimitLoadBalancer` with a custom interval duration.
    ///
    /// # Arguments
    ///
    /// * `entries` - A vector of tuples `(max_count, value)`.
    /// * `interval` - Duration after which allocation counts are reset.
    pub fn new_interval(entries: Vec<(u64, T)>, interval: Duration) -> Self {
        Self {
            inner: Arc::new(LimitLoadBalancerRef {
                entries: entries
                    .into_iter()
                    .map(|(max_count, value)| Entry {
                        max_count,
                        value,
                        count: 0.into(),
                    })
                    .collect::<Vec<_>>()
                    .into(),
                timer: Mutex::new(None),
                interval: interval.into(),
            }),
        }
    }

    /// Update the load balancer using an async callback.
    pub async fn update<F, R>(&self, handle: F) -> anyhow::Result<()>
    where
        F: Fn(Arc<LimitLoadBalancerRef<T>>) -> R,
        R: Future<Output = anyhow::Result<()>>,
    {
        handle(self.inner.clone()).await
    }

    /// Asynchronously allocate an entry, skipping the specified index.
    /// Loops until a valid entry is found.
    pub async fn alloc_skip(&self, index: usize) -> (usize, T) {
        loop {
            match self.try_alloc_skip(index) {
                Some(v) => return v,
                _ => yield_now().await,
            };
        }
    }

    /// Try to allocate an entry without awaiting.
    /// Returns `None` immediately if no entry is available.
    pub fn try_alloc_skip(&self, index: usize) -> Option<(usize, T)> {
        if let Ok(mut v) = self.inner.timer.try_lock() {
            if v.is_none() {
                let this = self.inner.clone();

                *v = Some(spawn(async move {
                    let mut interval = *this.interval.read().await;

                    loop {
                        sleep(match this.interval.try_read() {
                            Ok(v) => {
                                interval = *v;
                                interval
                            }
                            Err(_) => interval,
                        })
                        .await;

                        for i in this.entries.read().await.iter() {
                            i.count.store(0, Release);
                        }
                    }
                }));
            }
        }

        if let Ok(v) = self.inner.entries.try_read() {
            for (i, n) in v.iter().enumerate() {
                if i == index {
                    continue;
                }

                let count = n.count.load(Acquire);

                if n.max_count == 0
                    || count < n.max_count
                        && n.count
                            .compare_exchange(count, count + 1, Release, Acquire)
                            .is_ok()
                {
                    return Some((i, n.value.clone()));
                }
            }
        }

        None
    }
}

impl<T> LoadBalancer<T> for LimitLoadBalancer<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// Asynchronously allocate a resource from the load balancer.
    fn alloc(&self) -> impl Future<Output = T> + Send {
        async move { self.alloc_skip(usize::MAX).await.1 }
    }

    /// Synchronously try to allocate a resource.
    fn try_alloc(&self) -> Option<T> {
        self.try_alloc_skip(usize::MAX).map(|v| v.1)
    }
}

#[async_trait]
impl<T> BoxLoadBalancer<T> for LimitLoadBalancer<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// Asynchronously allocate a resource from the load balancer.
    async fn alloc(&self) -> T {
        self.alloc_skip(usize::MAX).await.1
    }

    /// Synchronously try to allocate a resource.
    fn try_alloc(&self) -> Option<T> {
        self.try_alloc_skip(usize::MAX).map(|v| v.1)
    }
}
