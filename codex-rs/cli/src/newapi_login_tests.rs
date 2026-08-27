use super::*;
use codex_http_client::HttpClientBuilder;
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn test_client(base_url: &str) -> NewApiClient {
    let http = HttpClientBuilder::new()
        .build_direct()
        .expect("build direct client");
    NewApiClient::new(http, base_url.to_string())
}

#[test]
fn normalizes_base_url() {
    assert_eq!(normalize_base_url("https://newapi.example.com/"), "https://newapi.example.com");
    assert_eq!(
        normalize_base_url("https://newapi.example.com/v1/"),
        "https://newapi.example.com"
    );
    assert_eq!(
        normalize_base_url("https://newapi.example.com/v1"),
        "https://newapi.example.com"
    );
}

#[test]
fn masks_keys() {
    assert_eq!(mask_key("shor"), "***");
    assert_eq!(mask_key("sk-1234567890abcdef"), "sk-123***cdef");
}

#[test]
fn builds_six_provider_edits() {
    let edits = build_newapi_provider_edits("https://newapi.example.com", "sk-test", "gpt-4o");
    assert_eq!(edits.len(), 6);
}

#[tokio::test]
async fn login_parses_token_and_user() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/user/login"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "data": {
                "token": "user-token",
                "user": { "id": 42 }
            }
        })))
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let (token, user_id) = client.login("alice", "secret").await.expect("login");
    assert_eq!(token, "user-token");
    assert_eq!(user_id, 42);
}

#[tokio::test]
async fn login_returns_error_message_on_failure() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/user/login"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": false,
            "message": "invalid credentials"
        })))
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let err = client.login("alice", "bad").await.expect_err("should fail");
    assert_eq!(err, "invalid credentials");
}

#[tokio::test]
async fn list_tokens_sends_auth_headers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/token/"))
        .and(header("Authorization", "Bearer user-token"))
        .and(header("New-Api-User", "7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "data": {
                "items": [
                    {
                        "id": 1,
                        "name": "primary",
                        "key": "sk-abc123def456",
                        "unlimited_quota": false,
                        "remain_quota": 5000000,
                        "expired_time": -1
                    }
                ]
            }
        })))
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let tokens = client
        .list_tokens("user-token", 7)
        .await
        .expect("list tokens");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].name, "primary");
    assert_eq!(tokens[0].key.as_deref(), Some("sk-abc123def456"));
}

#[tokio::test]
async fn create_token_returns_key() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/token/"))
        .and(header("Authorization", "Bearer user-token"))
        .and(header("New-Api-User", "7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "data": {
                "key": "sk-new-123456",
                "id": 9
            }
        })))
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let request = CreateTokenRequest {
        name: "codex".to_string(),
        expired_time: -1,
        remain_quota: 5_000_000,
        unlimited_quota: false,
        model_limits_enabled: false,
        model_limits: Vec::new(),
        allow_ips: String::new(),
        group: String::new(),
    };
    let key = client
        .create_token("user-token", 7, &request)
        .await
        .expect("create token");
    assert_eq!(key, "sk-new-123456");
}

#[tokio::test]
async fn list_tokens_supports_plain_array_shape() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/token/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "data": [
                { "id": 1, "name": "primary", "key": "sk-abc123def456" }
            ]
        })))
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let tokens = client.list_tokens("user-token", 7).await.expect("list tokens");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].name, "primary");
}
