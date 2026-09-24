use reqwest::header::HeaderValue;
use reqwest_middleware::Middleware;
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use serde::{Deserialize, Deserializer};
use std::{fmt, fmt::Formatter, sync::Arc, time::Duration};

#[derive(Clone, Deserialize)]
pub struct Config {
    pub url: url::Url,
    #[serde(default, deserialize_with = "deserialize_api_key")]
    pub api_key: Option<HeaderValue>,
    /// The maximum time limit for an API request. If a request takes longer than this, it will be
    /// cancelled. Defaults to 30 seconds.
    #[serde(default = "defaults::http_timeout")]
    pub http_timeout: Duration,
    #[serde(default)]
    pub probe_url: bool,
    #[serde(skip_deserializing)]
    pub middlewares: Vec<Arc<dyn Middleware>>,
}

fn deserialize_api_key<'de, D>(deserializer: D) -> Result<Option<HeaderValue>, D::Error>
where
    D: Deserializer<'de>,
{
    let string = Option::<String>::deserialize(deserializer)?;
    string
        .map(|value| HeaderValue::from_str(&value))
        .transpose()
        .map_err(<D::Error as serde::de::Error>::custom)
}

// We have to derive `Debug` manually as we need to skip middlewares field which does not implement it.
impl fmt::Debug for Config {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        #[derive(Debug)]
        #[allow(dead_code)]
        struct ConfigDebug<'a> {
            url: &'a url::Url,
            api_key: &'a Option<HeaderValue>,
            http_timeout: &'a Duration,
            probe_url: &'a bool,
        }
        let Config {
            url,
            api_key,
            http_timeout,
            probe_url,
            middlewares: _,
        } = self;
        fmt::Debug::fmt(
            &ConfigDebug {
                url,
                api_key,
                http_timeout,
                probe_url,
            },
            f,
        )
    }
}

impl Config {
    pub fn new(url: url::Url) -> Self {
        Self {
            url,
            api_key: None,
            http_timeout: defaults::http_timeout(),
            middlewares: vec![],
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
        self.middlewares.push(middleware);
        self
    }

    pub fn probe_url(mut self, value: bool) -> Self {
        self.probe_url = value;
        self
    }

    pub fn api_key(mut self, api_key: Option<HeaderValue>) -> Self {
        self.api_key = api_key;
        self
    }

    pub fn http_timeout(mut self, timeout: Duration) -> Self {
        self.http_timeout = timeout;
        self
    }
}

mod defaults {
    use std::time::Duration;

    pub fn http_timeout() -> Duration {
        Duration::from_secs(30)
    }
}
