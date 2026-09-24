use serde_json::json;
use wiremock::{
    matchers::{header_regex, method, path, query_param},
    Mock, MockServer, ResponseTemplate,
};

pub struct MockUser {
    pub id: i64,
    pub email: String,
    pub chain_id: i64,
    pub jwt: String,
    pub csrf_token: String,
}

pub async fn init_mocked_blockscout_auth_service(
    api_key: Option<&str>,
    users: &[MockUser],
) -> MockServer {
    let mock = MockServer::start().await;
    for u in users {
        let url = "/api/account/v2/authenticate";
        let mut m = Mock::given(method("GET"))
            .and(header_regex("cookie", &format!("_explorer_key={}", u.jwt)))
            .and(path(url));
        if let Some(key) = api_key {
            m = m.and(query_param("api_key", key));
        }
        m.respond_with(auth_resp(u)).mount(&mock).await;

        let mut m2 = Mock::given(method("POST"))
            .and(header_regex("cookie", &format!("_explorer_key={}", u.jwt)))
            .and(header_regex("x-csrf-token", &u.csrf_token))
            .and(path(url));
        if let Some(key) = api_key {
            m2 = m2.and(query_param("api_key", key));
        }
        m2.respond_with(auth_resp(u)).mount(&mock).await;
    }

    for u in users {
        let url = "/api/account/v2/user/info";
        let mut m = Mock::given(method("GET"))
            .and(header_regex("cookie", &format!("_explorer_key={}", u.jwt)))
            .and(path(url));
        if let Some(key) = api_key {
            m = m.and(query_param("api_key", key));
        }
        m.respond_with(userinfo_resp(u)).mount(&mock).await;
    }

    mock
}

fn auth_resp(user: &MockUser) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "avatar": "https://lh3.googleusercontent.com/a/image",
        "email": user.email,
        "id": user.id,
        "name": "User",
        "nickname": "username",
        "uid": "google-oauth2|10238912614929394",
        "watchlist_id": user.id,
        "email_verified": true
    }))
}

fn userinfo_resp(u: &MockUser) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "address_hash": format!("0x{:x}", u.id),
        "avatar": "https://cdn.auth0.com/avatars/0x.png",
        "email": u.email,
        "name": "User",
        "nickname": "username",
    }))
}
