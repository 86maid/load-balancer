use crate::{BoxLoadBalancer, LoadBalancer, interval::IntervalLoadBalancer};
use async_trait::async_trait;
use futures::future::join_all;
use get_if_addrs::get_if_addrs;
use reqwest::{Client, ClientBuilder, Proxy};
use std::{net::IpAddr, sync::Arc, time::Duration};

/// Load balancer for `reqwest::Client` instances bound to specific IP addresses.
/// Uses interval-based allocation.
#[derive(Clone)]
pub struct IPClient {
    inner: IntervalLoadBalancer<Client>,
}

impl IPClient {
    /// Create a new interval-based load balancer with given clients.
    pub fn new(entries: Vec<(Duration, Client)>) -> Self {
        Self {
            inner: IntervalLoadBalancer::new(entries),
        }
    }

    /// Build a load balancer using all local IP addresses.
    pub fn with_ip(ip: Vec<IpAddr>, interval: Duration) -> Self {
        Self {
            inner: IntervalLoadBalancer::new(
                ip.into_iter()
                    .map(|v| {
                        (
                            interval,
                            ClientBuilder::new().local_address(v).build().unwrap(),
                        )
                    })
                    .collect(),
            ),
        }
    }

    /// Build a load balancer using IP addresses with a per-client timeout.
    pub fn with_timeout(ip: Vec<IpAddr>, interval: Duration, timeout: Duration) -> Self {
        Self {
            inner: IntervalLoadBalancer::new(
                ip.into_iter()
                    .map(|v| {
                        (
                            interval,
                            ClientBuilder::new()
                                .local_address(v)
                                .timeout(timeout)
                                .build()
                                .unwrap(),
                        )
                    })
                    .collect(),
            ),
        }
    }

    /// Build a load balancer using IP addresses with a per-client timeout, use proxy.
    pub fn with_timeout_proxy(
        ip: Vec<IpAddr>,
        interval: Duration,
        timeout: Duration,
        proxy: Proxy,
    ) -> Self {
        Self {
            inner: IntervalLoadBalancer::new(
                ip.into_iter()
                    .map(|v| {
                        (
                            interval,
                            ClientBuilder::new()
                                .local_address(v)
                                .timeout(timeout)
                                .proxy(proxy.clone())
                                .build()
                                .unwrap(),
                        )
                    })
                    .collect(),
            ),
        }
    }

    /// Update the internal load balancer.
    pub async fn update<F, R>(&self, handle: F) -> anyhow::Result<()>
    where
        F: Fn(Arc<std::sync::RwLock<Vec<crate::interval::Entry<Client>>>>) -> R,
        R: std::future::Future<Output = anyhow::Result<()>>,
    {
        self.inner.update(handle).await
    }
}

impl LoadBalancer<Client> for IPClient {
    fn alloc(&self) -> impl std::future::Future<Output = Client> + Send {
        LoadBalancer::alloc(&self.inner)
    }

    fn try_alloc(&self) -> Option<Client> {
        LoadBalancer::try_alloc(&self.inner)
    }
}

#[async_trait]
impl BoxLoadBalancer<Client> for IPClient {
    async fn alloc(&self) -> Client {
        LoadBalancer::alloc(self).await
    }

    fn try_alloc(&self) -> Option<Client> {
        LoadBalancer::try_alloc(self)
    }
}

/// Get all non-loopback IP addresses of the machine.
pub fn get_ip_list() -> anyhow::Result<Vec<IpAddr>> {
    Ok(get_if_addrs()?
        .into_iter()
        .filter(|v| !v.is_loopback())
        .map(|v| v.ip())
        .collect::<Vec<_>>())
}

/// Get all non-loopback IPv4 addresses of the machine.
pub fn get_ipv4_list() -> anyhow::Result<Vec<IpAddr>> {
    Ok(get_if_addrs()?
        .into_iter()
        .filter(|v| !v.is_loopback() && v.ip().is_ipv4())
        .map(|v| v.ip())
        .collect::<Vec<_>>())
}

/// Get all non-loopback IPv6 addresses of the machine.
pub fn get_ipv6_list() -> anyhow::Result<Vec<IpAddr>> {
    Ok(get_if_addrs()?
        .into_iter()
        .filter(|v| !v.is_loopback() && v.ip().is_ipv6())
        .map(|v| v.ip())
        .collect::<Vec<_>>())
}

pub async fn test_ip(ip: IpAddr) -> anyhow::Result<IpAddr> {
    reqwest::ClientBuilder::new()
        .timeout(Duration::from_secs(3))
        .local_address(ip)
        .build()?
        .get("https://bilibili.com")
        .send()
        .await?;

    Ok(ip)
}

pub async fn test_all_ip() -> Vec<anyhow::Result<IpAddr>> {
    match get_ip_list() {
        Ok(v) => join_all(v.into_iter().map(|v| test_ip(v))).await,
        Err(_) => Vec::new(),
    }
}

pub async fn test_all_ipv4() -> Vec<anyhow::Result<IpAddr>> {
    match get_ipv4_list() {
        Ok(v) => join_all(v.into_iter().map(|v| test_ip(v))).await,
        Err(_) => Vec::new(),
    }
}

pub async fn test_all_ipv6() -> Vec<anyhow::Result<IpAddr>> {
    match get_ipv6_list() {
        Ok(v) => join_all(v.into_iter().map(|v| test_ip(v))).await,
        Err(_) => Vec::new(),
    }
}
