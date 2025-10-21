use crate::{
    BoxLoadBalancer, LoadBalancer,
    simple::{Entry, SimpleLoadBalancer, SimpleLoadBalancerRef},
};
use async_trait::async_trait;
use reqwest::Proxy;
use std::{
    ops::Range,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use tokio::{
    spawn,
    sync::Semaphore,
    task::JoinHandle,
    time::{Instant, sleep},
};

/// An advanced proxy pool that measures latency, removes dead proxies,
/// and sorts proxies by response time in ascending order.
#[derive(Clone)]
pub struct ProxyPool {
    code_range: Range<u16>,
    test_url: String,
    timeout: Duration,
    proxy: Option<Proxy>,
    max_check_concurrency: usize,
    lb: SimpleLoadBalancer<Arc<str>>,
}

impl ProxyPool {
    /// Create a new `LatencyProxyPool` from a list of proxy URLs.
    pub fn new<T: IntoIterator<Item = impl AsRef<str>>>(url: T) -> Self {
        Self {
            code_range: (200..300),
            test_url: "https://apple.com".to_string(),
            timeout: Duration::from_secs(5),
            proxy: None,
            max_check_concurrency: 1000,
            lb: SimpleLoadBalancer::new(url.into_iter().map(|v| v.as_ref().into()).collect()),
        }
    }

    /// Set the range of HTTP status codes that are considered successful.
    pub fn code_range(mut self, code_range: Range<u16>) -> Self {
        self.code_range = code_range;
        self
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

    /// Set the maximum number of concurrent proxy checks during health validation.
    pub fn max_check_concurrency(mut self, max_check_concurrency: usize) -> Self {
        self.max_check_concurrency = max_check_concurrency;
        self
    }

    /// Get the number of currently available (healthy) proxies.
    pub async fn available_count(&self) -> usize {
        self.lb
            .update(async |v| Ok(v.entries.read().await.len()))
            .await
            .unwrap()
    }

    /// Get available proxies.
    pub async fn available(&self) -> Vec<String> {
        self.lb
            .update(async |v| {
                Ok(v.entries
                    .read()
                    .await
                    .iter()
                    .map(|v| v.value.to_string())
                    .collect::<Vec<_>>())
            })
            .await
            .unwrap()
    }

    /// Add new proxies to the pool without performing immediate validation.
    ///
    /// New entries are appended, the cursor is reset, and the available count is updated.
    /// Validation occurs on the next `check()` call.
    pub async fn extend<T: IntoIterator<Item = impl AsRef<str>>>(&self, urls: T) {
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

                Ok(())
            })
            .await
            .unwrap();
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

                let result = self.internal_check(&old_entries, retry_count).await?;

                let mut new_entries = Vec::with_capacity(result.len());

                for (index, _) in result {
                    new_entries.push(old_entries[index].clone());
                }

                let mut lock = v.entries.write().await;

                *lock = new_entries;
                v.cursor.store(0, Ordering::Relaxed);

                Ok(())
            })
            .await
    }

    /// Validate all proxies, remove dead ones, and sort by latency.
    pub async fn check(&self, retry_count: usize) -> anyhow::Result<()> {
        self.lb
            .update(async |v| {
                let old_entries = v.entries.read().await;

                let result = self.internal_check(&old_entries, retry_count).await?;

                let mut new_entries = Vec::with_capacity(result.len());

                for (index, _) in result {
                    new_entries.push(old_entries[index].clone());
                }

                drop(old_entries);

                let mut lock = v.entries.write().await;

                *lock = new_entries;
                v.cursor.store(0, Ordering::Relaxed);

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

    /// Spawn a background task with a callback after each check.
    pub async fn spawn_check_callback<F, R>(
        &self,
        check_interval: Duration,
        retry_count: usize,
        callback: F,
    ) -> anyhow::Result<JoinHandle<anyhow::Result<()>>>
    where
        F: Fn() -> R + Send + 'static,
        R: Future<Output = anyhow::Result<()>> + Send,
    {
        self.check(retry_count).await?;
        callback().await?;

        let this = self.clone();

        Ok(spawn(async move {
            loop {
                sleep(check_interval).await;
                _ = this.check(retry_count).await;
                callback().await?;
            }
        }))
    }

    /// Update the load balancer using a custom async handler.
    pub async fn update<F, R>(&self, handle: F) -> anyhow::Result<()>
    where
        F: Fn(Arc<SimpleLoadBalancerRef<Arc<str>>>) -> R,
        R: Future<Output = anyhow::Result<()>>,
    {
        self.lb.update(handle).await
    }

    async fn internal_check(
        &self,
        entries: &Vec<Entry<Arc<str>>>,
        retry_count: usize,
    ) -> anyhow::Result<Vec<(usize, u128)>> {
        let semaphore = Arc::new(Semaphore::new(self.max_check_concurrency));
        let mut task = Vec::with_capacity(entries.len());

        for (index, entry) in entries.iter().enumerate() {
            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let entry = entry.clone();
            let code_range = self.code_range.clone();
            let test_url = self.test_url.clone();
            let timeout = self.timeout;
            let upstream_proxy = self.proxy.clone();
            let entry_value = entry.value.clone();

            task.push(tokio::spawn(async move {
                let _permit = permit;
                let mut latency = None;

                for _ in 0..=retry_count {
                    let client = if let Some(proxy) = upstream_proxy.clone() {
                        reqwest::ClientBuilder::new()
                            .proxy(proxy)
                            .proxy(Proxy::all(&*entry_value)?)
                            .timeout(timeout)
                            .build()?
                    } else {
                        reqwest::ClientBuilder::new()
                            .proxy(Proxy::all(&*entry_value)?)
                            .timeout(timeout)
                            .build()?
                    };

                    let start = Instant::now();

                    if let Ok(v) = client.get(&test_url).send().await {
                        if code_range.contains(&v.status().as_u16()) {
                            latency = Some(start.elapsed().as_millis());
                            break;
                        }
                    }
                }

                anyhow::Ok(latency.map(|v| (index, v)))
            }));
        }

        let mut result = Vec::new();

        for i in task {
            if let Ok(Ok(Some(r))) = i.await {
                result.push(r);
            }
        }

        result.sort_by_key(|(_, latency)| *latency);

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
