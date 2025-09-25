use crate::{BoxLoadBalancer, LoadBalancer};
use async_trait::async_trait;
use std::sync::atomic::Ordering::{Acquire, Release};
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

/// Represents a single entry in the threshold load balancer.
/// Tracks the maximum allowed requests, maximum errors, current usage count, and error count.
pub struct Entry<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// Maximum number of allowed allocations per interval.
    pub max_count: u64,
    /// Maximum number of allowed errors before the entry is considered disabled.
    pub max_error_count: u64,
    /// Current allocation count.
    pub count: AtomicU64,
    /// Current error count.
    pub error_count: AtomicU64,
    /// The actual value being balanced (e.g., client, resource).
    pub value: T,
}

impl<T> Entry<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// Reset both the allocation count and the error count.
    pub fn reset(&self) {
        self.count.store(0, Release);
        self.error_count.store(0, Release);
    }

    /// Disable this entry by setting its error count to the maximum.
    pub fn disable(&self) {
        self.error_count.store(self.max_error_count, Release);
    }
}

impl<T> Clone for Entry<T>
where
    T: Send + Sync + Clone + 'static,
{
    fn clone(&self) -> Self {
        Self {
            max_count: self.max_count.clone(),
            max_error_count: self.max_error_count.clone(),
            count: self.count.load(Acquire).into(),
            error_count: self.error_count.load(Acquire).into(),
            value: self.value.clone(),
        }
    }
}

/// Internal representation of the threshold load balancer.
pub struct ThresholdLoadBalancerRef<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// List of entries to balance between.
    pub entries: RwLock<Vec<Entry<T>>>,
    /// Optional background timer handle for resetting counts periodically.
    pub timer: Mutex<Option<JoinHandle<()>>>,
    /// Interval duration for resetting counts.
    pub interval: RwLock<Duration>,
}

/// Threshold-based load balancer that limits allocations per entry and handles failures.
#[derive(Clone)]
pub struct ThresholdLoadBalancer<T>
where
    T: Send + Sync + Clone + 'static,
{
    inner: Arc<ThresholdLoadBalancerRef<T>>,
}

impl<T> ThresholdLoadBalancer<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// Create a new threshold load balancer with a fixed interval.
    pub fn new(entries: Vec<(u64, u64, T)>, interval: Duration) -> Self {
        Self {
            inner: Arc::new(ThresholdLoadBalancerRef {
                entries: entries
                    .into_iter()
                    .map(|(max_count, max_error_count, value)| Entry {
                        max_count,
                        max_error_count,
                        value,
                        count: 0.into(),
                        error_count: 0.into(),
                    })
                    .collect::<Vec<_>>()
                    .into(),
                timer: Mutex::new(None),
                interval: interval.into(),
            }),
        }
    }

    /// Create a new threshold load balancer with a custom interval.
    pub fn new_interval(entries: Vec<(u64, u64, T)>, interval: Duration) -> Self {
        Self::new(entries, interval)
    }

    /// Execute a custom async update on the internal reference.
    pub async fn update<F, R>(&self, handle: F) -> anyhow::Result<()>
    where
        F: Fn(Arc<ThresholdLoadBalancerRef<T>>) -> R,
        R: Future<Output = anyhow::Result<()>>,
    {
        handle(self.inner.clone()).await
    }

    /// Allocate an entry, skipping the specified index if provided.
    async fn alloc_skip(&self, skip_index: usize) -> (usize, T) {
        loop {
            match self.try_alloc_skip(skip_index) {
                Some(v) => return v,
                None => yield_now().await,
            };
        }
    }

    /// Try to allocate an entry immediately, skipping the specified index if provided.
    fn try_alloc_skip(&self, skip_index: usize) -> Option<(usize, T)> {
        // Start the background timer if it is not already running.
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

                        // Reset the allocation count for all entries.
                        for i in this.entries.read().await.iter() {
                            i.count.store(0, Release);
                        }
                    }
                }));
            }
        }

        // Attempt to select a valid entry.
        if let Ok(entries) = self.inner.entries.try_read() {
            let mut skip_count = 0;

            for (i, entry) in entries.iter().enumerate() {
                if i == skip_index {
                    continue;
                }

                if entry.max_error_count != 0
                    && entry.error_count.load(Acquire) >= entry.max_error_count
                {
                    skip_count += 1;
                    continue;
                }

                let count = entry.count.load(Acquire);

                if entry.max_count == 0
                    || (count < entry.max_count
                        && entry
                            .count
                            .compare_exchange(count, count + 1, Release, Acquire)
                            .is_ok())
                {
                    return Some((i, entry.value.clone()));
                }
            }

            // All entries are skipped due to errors.
            if skip_count == entries.len() {
                return None;
            }
        }

        None
    }

    /// Mark a successful usage for the entry at the given index.
    pub fn success(&self, index: usize) {
        if let Ok(entries) = self.inner.entries.try_read() {
            if let Some(entry) = entries.get(index) {
                let current = entry.error_count.load(Acquire);

                if current != 0 {
                    let _ =
                        entry
                            .error_count
                            .compare_exchange(current, current - 1, Release, Acquire);
                }
            }
        }
    }

    /// Mark a failure for the entry at the given index.
    pub fn failure(&self, index: usize) {
        if let Ok(entries) = self.inner.entries.try_read() {
            if let Some(entry) = entries.get(index) {
                entry.error_count.fetch_add(1, Release);
            }
        }
    }
}

impl<T> LoadBalancer<T> for ThresholdLoadBalancer<T>
where
    T: Clone + Send + Sync + 'static,
{
    /// Allocate an entry asynchronously.
    fn alloc(&self) -> impl Future<Output = T> + Send {
        async move { self.alloc_skip(usize::MAX).await.1 }
    }

    /// Attempt to allocate an entry immediately.
    fn try_alloc(&self) -> Option<T> {
        self.try_alloc_skip(usize::MAX).map(|v| v.1)
    }
}

#[async_trait]
impl<T> BoxLoadBalancer<T> for ThresholdLoadBalancer<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// Allocate an entry asynchronously.
    async fn alloc(&self) -> T {
        self.alloc_skip(usize::MAX).await.1
    }

    /// Attempt to allocate an entry immediately.
    fn try_alloc(&self) -> Option<T> {
        self.try_alloc_skip(usize::MAX).map(|v| v.1)
    }
}
