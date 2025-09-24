use crate::{
    BoxLoadBalancer, LoadBalancer,
    limit::{LimitLoadBalancer, LimitLoadBalancerRef},
};
use async_trait::async_trait;
use get_if_addrs::get_if_addrs;
use reqwest::{Client, ClientBuilder};
use std::{net::IpAddr, sync::Arc, time::Duration};

/// Load balancer for `reqwest::Client` instances bound to specific IP addresses.
/// Supports limits per client and optional interval-based resets.
#[derive(Clone)]
pub struct IPClientLoadBalancer {
    inner: LimitLoadBalancer<Client>,
}

impl IPClientLoadBalancer {
    /// Create a new load balancer with fixed clients and limits.
    pub fn new(entries: Vec<(u64, Client)>) -> Self {
        Self {
            inner: LimitLoadBalancer::new(entries),
        }
    }

    /// Create a new load balancer with a custom interval for resetting limits.
    pub fn new_interval(entries: Vec<(u64, Client)>, interval: Duration) -> Self {
        Self {
            inner: LimitLoadBalancer::new_interval(entries, interval),
        }
    }

    /// Build a load balancer using all local IPv4 addresses.
    pub fn with_ipv4(limit: u64) -> Self {
        Self {
            inner: LimitLoadBalancer::new(
                get_ipv4_list()
                    .clone()
                    .into_iter()
                    .map(|v| ClientBuilder::new().local_address(v).build().unwrap())
                    .map(move |v| (limit, v))
                    .collect(),
            ),
        }
    }

    /// Build a load balancer using IPv4 addresses with a custom interval.
    pub fn with_ipv4_interval(limit: u64, interval: Duration) -> Self {
        Self {
            inner: LimitLoadBalancer::new_interval(
                get_ipv4_list()
                    .clone()
                    .into_iter()
                    .map(|v| ClientBuilder::new().local_address(v).build().unwrap())
                    .map(move |v| (limit, v))
                    .collect(),
                interval,
            ),
        }
    }

    /// Build a load balancer using all local IPv6 addresses.
    pub fn with_ipv6(limit: u64) -> Self {
        Self {
            inner: LimitLoadBalancer::new(
                get_ipv6_list()
                    .clone()
                    .into_iter()
                    .map(|v| ClientBuilder::new().local_address(v).build().unwrap())
                    .map(move |v| (limit, v))
                    .collect(),
            ),
        }
    }

    /// Build a load balancer using IPv6 addresses with a custom interval.
    pub fn with_ipv6_interval(limit: u64, interval: Duration) -> Self {
        Self {
            inner: LimitLoadBalancer::new_interval(
                get_ipv6_list()
                    .clone()
                    .into_iter()
                    .map(|v| ClientBuilder::new().local_address(v).build().unwrap())
                    .map(move |v| (limit, v))
                    .collect(),
                interval,
            ),
        }
    }

    /// Build a load balancer using IPv4 addresses with a per-client timeout.
    pub fn with_ipv4_timeout(limit: u64, timeout: Duration) -> Self {
        Self {
            inner: LimitLoadBalancer::new(
                get_ipv4_list()
                    .clone()
                    .into_iter()
                    .map(|v| {
                        ClientBuilder::new()
                            .local_address(v)
                            .timeout(timeout)
                            .build()
                            .unwrap()
                    })
                    .map(move |v| (limit, v))
                    .collect(),
            ),
        }
    }

    /// Build a load balancer using IPv4 addresses with interval and timeout.
    pub fn with_ipv4_interval_timeout(limit: u64, interval: Duration, timeout: Duration) -> Self {
        Self {
            inner: LimitLoadBalancer::new_interval(
                get_ipv4_list()
                    .clone()
                    .into_iter()
                    .map(|v| {
                        ClientBuilder::new()
                            .local_address(v)
                            .timeout(timeout)
                            .build()
                            .unwrap()
                    })
                    .map(move |v| (limit, v))
                    .collect(),
                interval,
            ),
        }
    }

    /// Build a load balancer using IPv6 addresses with a per-client timeout.
    pub fn with_ipv6_timeout(limit: u64, timeout: Duration) -> Self {
        Self {
            inner: LimitLoadBalancer::new(
                get_ipv6_list()
                    .clone()
                    .into_iter()
                    .map(|v| {
                        ClientBuilder::new()
                            .local_address(v)
                            .timeout(timeout)
                            .build()
                            .unwrap()
                    })
                    .map(move |v| (limit, v))
                    .collect(),
            ),
        }
    }

    /// Build a load balancer using IPv6 addresses with interval and timeout.
    pub fn with_ipv6_interval_timeout(limit: u64, interval: Duration, timeout: Duration) -> Self {
        Self {
            inner: LimitLoadBalancer::new_interval(
                get_ipv6_list()
                    .clone()
                    .into_iter()
                    .map(|v| {
                        ClientBuilder::new()
                            .local_address(v)
                            .timeout(timeout)
                            .build()
                            .unwrap()
                    })
                    .map(move |v| (limit, v))
                    .collect(),
                interval,
            ),
        }
    }

    /// Update the internal load balancer using a custom async closure.
    pub async fn update<F, R>(&self, handle: F) -> anyhow::Result<()>
    where
        F: Fn(Arc<LimitLoadBalancerRef<Client>>) -> R,
        R: Future<Output = anyhow::Result<()>>,
    {
        self.inner.update(handle).await
    }
}

impl LoadBalancer<Client> for IPClientLoadBalancer {
    /// Allocate a client asynchronously.
    fn alloc(&self) -> impl Future<Output = Client> + Send {
        LoadBalancer::alloc(&self.inner)
    }

    /// Attempt to allocate a client immediately.
    fn try_alloc(&self) -> Option<Client> {
        LoadBalancer::try_alloc(&self.inner)
    }
}

#[async_trait]
impl BoxLoadBalancer<Client> for IPClientLoadBalancer {
    /// Allocate a client asynchronously.
    async fn alloc(&self) -> Client {
        LoadBalancer::alloc(self).await
    }

    /// Attempt to allocate a client immediately.
    fn try_alloc(&self) -> Option<Client> {
        LoadBalancer::try_alloc(self)
    }
}

/// Get all non-loopback IP addresses of the machine.
pub fn get_ip_list() -> Vec<IpAddr> {
    get_if_addrs()
        .unwrap()
        .into_iter()
        .filter(|v| !v.is_loopback())
        .map(|v| v.ip())
        .collect::<Vec<_>>()
}

/// Get all non-loopback IPv4 addresses of the machine.
pub fn get_ipv4_list() -> Vec<IpAddr> {
    get_if_addrs()
        .unwrap()
        .into_iter()
        .filter(|v| !v.is_loopback() && v.ip().is_ipv4())
        .map(|v| v.ip())
        .collect::<Vec<_>>()
}

/// Get all non-loopback IPv6 addresses of the machine.
pub fn get_ipv6_list() -> Vec<IpAddr> {
    get_if_addrs()
        .unwrap()
        .into_iter()
        .filter(|v| !v.is_loopback() && v.ip().is_ipv6())
        .map(|v| v.ip())
        .collect::<Vec<_>>()
}
