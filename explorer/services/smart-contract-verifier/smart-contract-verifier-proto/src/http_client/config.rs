use reqwest_middleware::Middleware;
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use std::{str::FromStr, sync::Arc};

type BoxError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Clone)]
pub struct Config {
    url: String,
    middleware_stack: Vec<Arc<dyn Middleware>>,
    probe_url: bool,
}

impl Config {
    pub fn new(url: String) -> Self {
        Self {
            url,
            middleware_stack: vec![],
            probe_url: false,
        }
    }

    pub fn with_retry_middleware(self, max_retries: u32) -> Self {
        let retry_policy = ExponentialBackoff::builder().build_with_max_retries(max_retries);
        let middleware = RetryTransientMiddleware::new_with_policy(retry_policy);
        self.with_middleware(middleware)
    }

    pub fn with_middleware<M: Middleware>(self, middleware: M) -> Self {
        self.with_arc_middleware(Arc::new(middleware))
    }

    pub fn with_arc_middleware<M: Middleware>(mut self, middleware: Arc<M>) -> Self {
        self.middleware_stack.push(middleware);
        self
    }

    pub fn probe_url(mut self, value: bool) -> Self {
        self.probe_url = value;
        self
    }
}

#[derive(Clone)]
pub(super) struct ValidatedConfig {
    pub url: url::Url,
    pub middleware_stack: Vec<Arc<dyn Middleware>>,
    pub probe_url: bool,
}

impl TryFrom<Config> for ValidatedConfig {
    type Error = BoxError;

    fn try_from(value: Config) -> std::result::Result<Self, Self::Error> {
        let url = url::Url::from_str(&value.url)?;

        Ok(Self {
            url,
            middleware_stack: value.middleware_stack,
            probe_url: value.probe_url,
        })
    }
}
