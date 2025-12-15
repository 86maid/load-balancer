use crate::{BoxLoadBalancer, LoadBalancer};
use async_trait::async_trait;
use std::sync::atomic::Ordering::{Acquire, Release};
use std::{
    future::Future,
    sync::{Arc, atomic::AtomicU64},
    time::{Duration, Instant},
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
    /// Maximum number of allocations per interval.
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
pub struct WindowEcLoadBalancerRef<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// List of entries to balance between.
    pub entries: RwLock<Vec<Entry<T>>>,
    /// Optional background timer handle for resetting counts periodically.
    pub timer: Mutex<Option<JoinHandle<()>>>,
    /// Interval duration for resetting counts.
    pub interval: RwLock<Duration>,
    /// The next scheduled reset time.
    pub next_reset: RwLock<Instant>,
}

/// Threshold-based load balancer that limits allocations per entry and handles failures.
#[derive(Clone)]
pub struct WindowEcLoadBalancer<T>
where
    T: Send + Sync + Clone + 'static,
{
    inner: Arc<WindowEcLoadBalancerRef<T>>,
}

impl<T> WindowEcLoadBalancer<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// Create a new threshold load balancer with a fixed 1-second interval.
    ///
    /// # Arguments
    ///
    /// * `entries` - A vector of tuples `(max_count, max_error_count, value)`:
    ///     - `max_count`: Maximum number of allocations allowed per interval.
    ///     - `max_error_count`: Maximum number of errors allowed before disabling the entry.
    ///     - `value`: value.
    pub fn new(entries: Vec<(u64, u64, T)>) -> Self {
        Self::new_interval(entries, Duration::from_secs(1))
    }

    /// Create a new threshold load balancer with a custom interval.
    ///
    /// # Arguments
    ///
    /// * `entries` - A vector of tuples `(max_count, max_error_count, value)`:
    ///     - `max_count`: Maximum number of allocations allowed per interval.
    ///     - `max_error_count`: Maximum number of errors allowed before disabling the entry.
    ///     - `value`: value.
    ///
    /// * `interval` - Duration after which all allocation/error counts are reset.
    pub fn new_interval(entries: Vec<(u64, u64, T)>, interval: Duration) -> Self {
        Self {
            inner: Arc::new(WindowEcLoadBalancerRef {
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
                next_reset: RwLock::new(Instant::now() + interval),
            }),
        }
    }

    /// Execute a custom async update on the internal reference.
    pub async fn update<F, R>(&self, handle: F) -> anyhow::Result<()>
    where
        F: Fn(Arc<WindowEcLoadBalancerRef<T>>) -> R,
        R: Future<Output = anyhow::Result<()>>,
    {
        handle(self.inner.clone()).await
    }

    /// Allocate an entry, skipping the specified index if provided.
    pub async fn alloc_skip(&self, skip_index: usize) -> (usize, T) {
        loop {
            if let Some(v) = self.try_alloc_skip(skip_index) {
                return v;
            }

            let now = Instant::now();

            let next = *self.inner.next_reset.read().await;

            let remaining = if now < next {
                next - now
            } else {
                Duration::ZERO
            };

            if remaining > Duration::ZERO {
                sleep(remaining).await;
            } else {
                yield_now().await;
            }
        }
    }

    /// Try to allocate an entry immediately, skipping the specified index if provided.
    pub fn try_alloc_skip(&self, skip_index: usize) -> Option<(usize, T)> {
        if let Ok(mut timer_guard) = self.inner.timer.try_lock() {
            if timer_guard.is_none() {
                let this = self.inner.clone();

                *timer_guard = Some(spawn(async move {
                    let mut interval = *this.interval.read().await;

                    *this.next_reset.write().await = Instant::now() + interval;

                    loop {
                        sleep(match this.interval.try_read() {
                            Ok(v) => {
                                interval = *v;
                                interval
                            }
                            Err(_) => interval,
                        })
                        .await;

                        let now = Instant::now();

                        let entries = this.entries.read().await;

                        for entry in entries.iter() {
                            entry.count.store(0, Release);
                        }

                        *this.next_reset.write().await = now + interval;
                    }
                }));
            }
        }

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

impl<T> LoadBalancer<T> for WindowEcLoadBalancer<T>
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
impl<T> BoxLoadBalancer<T> for WindowEcLoadBalancer<T>
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
