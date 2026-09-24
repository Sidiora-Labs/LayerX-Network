use crate::launcher::ServerSettings;
use reqwest::Url;
use std::{
    future::Future,
    net::{SocketAddr, TcpListener},
    str::FromStr,
    time::Duration,
};
use tokio::{
    task::JoinHandle,
    time::{sleep, timeout},
};

fn get_free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

pub fn get_test_server_settings() -> (ServerSettings, Url) {
    let mut server = ServerSettings::default();
    let port = get_free_port();
    server.http.addr = SocketAddr::from_str(&format!("127.0.0.1:{port}")).unwrap();
    server.grpc.enabled = false;
    let base = Url::parse(&format!("http://{}", server.http.addr)).unwrap();
    (server, base)
}

/// Use [`TestServerSettings`] for more configurable interface
pub async fn init_server<F, R>(run: F, base: &Url) -> JoinHandle<Result<(), anyhow::Error>>
where
    F: FnOnce() -> R + Send + 'static,
    R: Future<Output = Result<(), anyhow::Error>> + Send,
{
    TestServerSettings::new(base.clone()).init(run).await
}

pub struct TestServerSettings {
    pub healthcheck_timeout: Duration,
    pub base: Url,
}

impl TestServerSettings {
    pub fn new(base: Url) -> Self {
        Self {
            healthcheck_timeout: Duration::from_secs(15),
            base,
        }
    }
}

impl TestServerSettings {
    pub async fn init<F, R>(self, run: F) -> JoinHandle<Result<(), anyhow::Error>>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Future<Output = Result<(), anyhow::Error>> + Send,
    {
        let server_handle = tokio::spawn(async move { run().await });

        let client = reqwest::Client::new();
        let health_endpoint = self.base.join("health").unwrap();

        let wait_health_check = async {
            loop {
                if let Ok(response) = client
                    .request(reqwest::Method::GET, health_endpoint.clone())
                    .query(&[("service", "")])
                    .send()
                    .await
                {
                    if response.status() == reqwest::StatusCode::OK {
                        break;
                    }
                }
                sleep(Duration::from_millis(100)).await;
            }
        };
        // Wait for the server to start
        if (timeout(self.healthcheck_timeout, wait_health_check).await).is_err() {
            match timeout(Duration::from_secs(1), server_handle).await {
                Ok(Ok(result)) => {
                    panic!("Server terminated with: {result:?}")
                }
                Ok(Err(_)) => {
                    panic!("Server start terminated with exit error")
                }
                Err(_) => {
                    panic!("Server did not start in time, and did not terminate");
                }
            }
        }

        server_handle
    }
}

async fn send_annotated_request<Response: for<'a> serde::Deserialize<'a>>(
    url: &Url,
    route: &str,
    method: reqwest::Method,
    payload: Option<&impl serde::Serialize>,
    annotation: Option<&str>,
) -> Response {
    let annotation = annotation.map(|v| format!("({v}) ")).unwrap_or_default();

    let mut request = reqwest::Client::new().request(method, url.join(route).unwrap());
    if let Some(p) = payload {
        request = request.json(p);
    };
    let response = request
        .send()
        .await
        .unwrap_or_else(|e| panic!("{annotation}Failed to send request: {}", e.without_url()));

    // Assert that status code is success
    if !response.status().is_success() {
        let status = response.status();
        let message = response.text().await.expect("Read body as text");
        panic!("({annotation})Invalid status code (success expected). Status: {status}. Message: {message}")
    }

    response
        .json()
        .await
        .unwrap_or_else(|_| panic!("({annotation})Response deserialization failed"))
}

pub async fn send_annotated_post_request<Response: for<'a> serde::Deserialize<'a>>(
    url: &Url,
    route: &str,
    payload: &impl serde::Serialize,
    annotation: &str,
) -> Response {
    send_annotated_request(
        url,
        route,
        reqwest::Method::POST,
        Some(payload),
        Some(annotation),
    )
    .await
}

pub async fn send_post_request<Response: for<'a> serde::Deserialize<'a>>(
    url: &Url,
    route: &str,
    payload: &impl serde::Serialize,
) -> Response {
    send_annotated_request(url, route, reqwest::Method::POST, Some(payload), None).await
}

pub async fn send_annotated_get_request<Response: for<'a> serde::Deserialize<'a>>(
    url: &Url,
    route: &str,
    annotation: &str,
) -> Response {
    send_annotated_request(
        url,
        route,
        reqwest::Method::GET,
        None::<&()>,
        Some(annotation),
    )
    .await
}

pub async fn send_get_request<Response: for<'a> serde::Deserialize<'a>>(
    url: &Url,
    route: &str,
) -> Response {
    send_annotated_request(url, route, reqwest::Method::GET, None::<&()>, None).await
}
