use crate::{BoxLoadBalancer, LoadBalancer, interval::IntervalLoadBalancer};
use async_trait::async_trait;
use get_if_addrs::get_if_addrs;
use reqwest::{Client, ClientBuilder};
use std::{net::IpAddr, sync::Arc, time::Duration};

/// Load balancer for `reqwest::Client` instances bound to specific IP addresses.
/// Uses interval-based allocation.
#[derive(Clone)]
pub struct IPClientLoadBalancer {
    inner: IntervalLoadBalancer<Client>,
}

impl IPClientLoadBalancer {
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

    /// Update the internal load balancer.
    pub async fn update<F, R>(&self, handle: F) -> anyhow::Result<()>
    where
        F: Fn(Arc<std::sync::RwLock<Vec<crate::interval::Entry<Client>>>>) -> R,
        R: std::future::Future<Output = anyhow::Result<()>>,
    {
        self.inner.update(handle).await
    }
}

impl LoadBalancer<Client> for IPClientLoadBalancer {
    fn alloc(&self) -> impl std::future::Future<Output = Client> + Send {
        LoadBalancer::alloc(&self.inner)
    }

    fn try_alloc(&self) -> Option<Client> {
        LoadBalancer::try_alloc(&self.inner)
    }
}

#[async_trait]
impl BoxLoadBalancer<Client> for IPClientLoadBalancer {
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
