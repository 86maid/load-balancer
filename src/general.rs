use crate::*;

#[derive(Clone)]
pub enum GeneralLoadBalancer<T>
where
    T: Send + Sync + Clone + 'static,
{
    Interval(interval::IntervalLoadBalancer<T>),
    Limit(limit::LimitLoadBalancer<T>),
    Random(random::RandomLoadBalancer<T>),
    Simple(simple::SimpleLoadBalancer<T>),
    Threshold(threshold::ThresholdLoadBalancer<T>),
}

impl<T> GeneralLoadBalancer<T>
where
    T: Send + Sync + Clone + 'static,
{
    pub fn as_interval(&self) -> &interval::IntervalLoadBalancer<T> {
        if let GeneralLoadBalancer::Interval(v) = self {
            v
        } else {
            panic!("called as_interval on non-Interval variant");
        }
    }

    pub fn as_limit(&self) -> &limit::LimitLoadBalancer<T> {
        if let GeneralLoadBalancer::Limit(v) = self {
            v
        } else {
            panic!("called as_limit on non-Limit variant");
        }
    }

    pub fn as_random(&self) -> &random::RandomLoadBalancer<T> {
        if let GeneralLoadBalancer::Random(v) = self {
            v
        } else {
            panic!("called as_random on non-Random variant");
        }
    }

    pub fn as_simple(&self) -> &simple::SimpleLoadBalancer<T> {
        if let GeneralLoadBalancer::Simple(v) = self {
            v
        } else {
            panic!("called as_simple on non-Simple variant");
        }
    }

    pub fn as_threshold(&self) -> &threshold::ThresholdLoadBalancer<T> {
        if let GeneralLoadBalancer::Threshold(v) = self {
            v
        } else {
            panic!("called as_threshold on non-Threshold variant");
        }
    }
}

impl<T> LoadBalancer<T> for GeneralLoadBalancer<T>
where
    T: Send + Sync + Clone + 'static,
{
    async fn alloc(&self) -> T {
        match self {
            GeneralLoadBalancer::Interval(v) => LoadBalancer::alloc(v).await,
            GeneralLoadBalancer::Limit(v) => LoadBalancer::alloc(v).await,
            GeneralLoadBalancer::Random(v) => LoadBalancer::alloc(v).await,
            GeneralLoadBalancer::Simple(v) => LoadBalancer::alloc(v).await,
            GeneralLoadBalancer::Threshold(v) => LoadBalancer::alloc(v).await,
        }
    }

    fn try_alloc(&self) -> Option<T> {
        match self {
            GeneralLoadBalancer::Interval(v) => LoadBalancer::try_alloc(v),
            GeneralLoadBalancer::Limit(v) => LoadBalancer::try_alloc(v),
            GeneralLoadBalancer::Random(v) => LoadBalancer::try_alloc(v),
            GeneralLoadBalancer::Simple(v) => LoadBalancer::try_alloc(v),
            GeneralLoadBalancer::Threshold(v) => LoadBalancer::try_alloc(v),
        }
    }
}

#[async_trait]
impl<T> BoxLoadBalancer<T> for GeneralLoadBalancer<T>
where
    T: Send + Sync + Clone + 'static,
{
    async fn alloc(&self) -> T {
        match self {
            GeneralLoadBalancer::Interval(v) => BoxLoadBalancer::alloc(v).await,
            GeneralLoadBalancer::Limit(v) => BoxLoadBalancer::alloc(v).await,
            GeneralLoadBalancer::Random(v) => BoxLoadBalancer::alloc(v).await,
            GeneralLoadBalancer::Simple(v) => BoxLoadBalancer::alloc(v).await,
            GeneralLoadBalancer::Threshold(v) => BoxLoadBalancer::alloc(v).await,
        }
    }

    fn try_alloc(&self) -> Option<T> {
        match self {
            GeneralLoadBalancer::Interval(v) => BoxLoadBalancer::try_alloc(v),
            GeneralLoadBalancer::Limit(v) => BoxLoadBalancer::try_alloc(v),
            GeneralLoadBalancer::Random(v) => BoxLoadBalancer::try_alloc(v),
            GeneralLoadBalancer::Simple(v) => BoxLoadBalancer::try_alloc(v),
            GeneralLoadBalancer::Threshold(v) => BoxLoadBalancer::try_alloc(v),
        }
    }
}
