use crate::{
    BoxLoadBalancer, LoadBalancer,
    simple::{Entry, SimpleLoadBalancer},
};
use async_trait::async_trait;
use reqwest::Proxy;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    spawn,
    task::JoinHandle,
    time::{Instant, sleep},
};

/// An advanced proxy pool that measures latency, removes dead proxies,
/// and sorts proxies by response time in ascending order.
#[derive(Clone)]
pub struct ProxyPool {
    test_url: String,
    proxy: Option<Proxy>,
    timeout: Duration,
    available_count: Arc<AtomicUsize>,
    lb: SimpleLoadBalancer<Arc<str>>,
}

impl ProxyPool {
    /// Create a new `LatencyProxyPool` from a list of proxy URLs.
    pub fn new<T: IntoIterator<Item = impl AsRef<str>>>(url: T) -> Self {
        Self {
            test_url: "https://bilibili.com".to_string(),
            proxy: None,
            timeout: Duration::from_secs(3),
            available_count: Arc::new(AtomicUsize::new(0)),
            lb: SimpleLoadBalancer::new(url.into_iter().map(|v| v.as_ref().into()).collect()),
        }
    }

    /// Set the URL used for testing proxy connectivity.
    pub fn test_url(mut self, test_url: String) -> Self {
        self.test_url = test_url;
        self
    }

    /// Set the request timeout for proxy testing.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set an optional upstream proxy for proxy validation.
    pub fn proxy(mut self, proxy: Proxy) -> Self {
        self.proxy = Some(proxy);
        self
    }

    /// Get the number of currently available (healthy) proxies.
    pub fn available_count(&self) -> usize {
        self.available_count.load(Ordering::Relaxed)
    }

    /// Add new proxies to the pool without performing immediate validation.
    ///
    /// New entries are appended, the cursor is reset, and the available count is updated.
    /// Validation occurs on the next `check()` call.
    pub async fn extend<T: IntoIterator<Item = impl AsRef<str>>>(
        &self,
        urls: T,
    ) -> anyhow::Result<()> {
        let new_entries = urls
            .into_iter()
            .map(|v| Entry {
                value: Arc::from(v.as_ref()),
            })
            .collect::<Vec<_>>();

        self.lb
            .update(async |v| {
                let mut lock = v.entries.write().await;

                lock.extend(new_entries.clone());
                v.cursor.store(0, Ordering::Relaxed);
                self.available_count.store(lock.len(), Ordering::Relaxed);

                Ok(())
            })
            .await
    }

    /// Add new proxies and immediately perform connectivity and latency checks.
    ///
    /// Proxies are validated, failed ones are removed, and remaining entries
    /// are sorted by latency (ascending).
    pub async fn extend_check<T: IntoIterator<Item = impl AsRef<str>>>(
        &self,
        url: T,
        retry_count: usize,
    ) -> anyhow::Result<()> {
        let new_entries = url
            .into_iter()
            .map(|v| Entry {
                value: Arc::from(v.as_ref()),
            })
            .collect::<Vec<Entry<Arc<str>>>>();

        self.lb
            .update(async |v| {
                let old_entries = {
                    let lock = v.entries.read().await;
                    let mut result = Vec::with_capacity(lock.len() + new_entries.len());

                    result.extend_from_slice(&new_entries);
                    result.extend(lock.iter().cloned());

                    result
                };

                let mut result = self.internal_check(&old_entries, retry_count).await?;

                result.sort_by_key(|(_, latency)| *latency);

                let mut new_entries = Vec::with_capacity(result.len());

                for (index, _) in result {
                    new_entries.push(old_entries[index].clone());
                }

                let mut lock = v.entries.write().await;

                *lock = new_entries;
                v.cursor.store(0, Ordering::Relaxed);
                self.available_count.store(lock.len(), Ordering::Relaxed);

                Ok(())
            })
            .await
    }

    /// Validate all proxies, remove dead ones, and sort by latency.
    pub async fn check(&self, retry_count: usize) -> anyhow::Result<()> {
        self.lb
            .update(async |v| {
                let old_entries = v.entries.read().await;

                let mut result = self.internal_check(&old_entries, retry_count).await?;

                result.sort_by_key(|(_, latency)| *latency);

                let mut new_entries = Vec::with_capacity(result.len());

                for (index, _) in result {
                    new_entries.push(old_entries[index].clone());
                }

                drop(old_entries);

                let mut lock = v.entries.write().await;

                *lock = new_entries;
                v.cursor.store(0, Ordering::Relaxed);
                self.available_count.store(lock.len(), Ordering::Relaxed);

                Ok(())
            })
            .await
    }

    /// Spawn a background task to periodically validate proxies and update order by latency.
    ///
    /// Returns a `JoinHandle` to allow cancellation or awaiting of the task.
    pub async fn spawn_check(
        &self,
        check_interval: Duration,
        retry_count: usize,
    ) -> anyhow::Result<JoinHandle<()>> {
        self.check(retry_count).await?;

        let this = self.clone();

        Ok(spawn(async move {
            loop {
                sleep(check_interval).await;
                _ = this.check(retry_count).await;
            }
        }))
    }

    async fn internal_check(
        &self,
        entries: &Vec<Entry<Arc<str>>>,
        retry_count: usize,
    ) -> anyhow::Result<Vec<(usize, u128)>> {
        let mut result = Vec::new();

        for (index, entry) in entries.iter().enumerate() {
            let mut latency = None;

            for _ in 0..=retry_count {
                let client = if let Some(proxy) = self.proxy.clone() {
                    reqwest::ClientBuilder::new()
                        .proxy(proxy)
                        .proxy(Proxy::all(&*entry.value)?)
                        .timeout(self.timeout)
                        .build()?
                } else {
                    reqwest::ClientBuilder::new()
                        .proxy(Proxy::all(&*entry.value)?)
                        .timeout(self.timeout)
                        .build()?
                };

                let start = Instant::now();

                if let Ok(v) = client.get(&self.test_url).send().await {
                    if v.status().is_success() {
                        latency = Some(start.elapsed().as_millis());
                        break;
                    }
                }
            }

            if let Some(v) = latency {
                result.push((index, v));
            }
        }

        Ok(result)
    }
}

impl LoadBalancer<String> for ProxyPool {
    async fn alloc(&self) -> String {
        LoadBalancer::alloc(&self.lb).await.to_string()
    }

    fn try_alloc(&self) -> Option<String> {
        LoadBalancer::try_alloc(&self.lb).map(|v| v.to_string())
    }
}

#[async_trait]
impl BoxLoadBalancer<String> for ProxyPool {
    async fn alloc(&self) -> String {
        LoadBalancer::alloc(self).await
    }

    fn try_alloc(&self) -> Option<String> {
        LoadBalancer::try_alloc(self)
    }
}
