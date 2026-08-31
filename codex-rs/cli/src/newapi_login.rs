//! New API (newapi) relay login command.
//!
//! This adds a self-contained `codex login --newapi` flow that lets the user
//! connect Codex to their own New API (newapi) relay. After authenticating with
//! an admin username/password (or a pre-created access token), the command can
//! list existing API keys, create a fresh one, and persist the selected key as a
//! custom `model_providers` entry so `codex` works out of the box.

use crate::login::load_config_or_exit;
use codex_core::config::edit::{ConfigEdit, ConfigEditsBuilder};
use codex_core::config::Config;
use codex_http_client::{ClientRouteClass, HttpClient, HttpClientFactory, OutboundProxyPolicy};
use codex_utils_cli::CliConfigOverrides;
use serde::Deserialize;
use serde::Serialize;
use std::io::{self, Write};
use toml_edit::value;

/// New API stores quota in units where 500_000 == $1.
const QUOTA_PER_DOLLAR: i64 = 500_000;

/// Default provider id written to `model_providers` and `model_provider`.
const PROVIDER_ID: &str = "newapi";
const PROVIDER_NAME: &str = "New API";
/// Default model id used when the operator does not supply one.
const DEFAULT_MODEL: &str = "gpt-4o";

/// Build-time relay URL override, compiled in via the `NEWAPI_BASE_URL`
/// environment variable. When set, the interactive login flow pre-fills this
/// value so end users only enter their username/password.
const DEFAULT_BASE_URL: Option<&str> = option_env!("NEWAPI_BASE_URL");

/// Default relay used when no `NEWAPI_BASE_URL` is baked in at build time.
const FALLBACK_BASE_URL: &str = "https://new.ahei.asia";

/// Default product display name used when no `NEWAPI_PRODUCT_NAME` is baked in.
const FALLBACK_PRODUCT_NAME: &str = "AHEIAPI";

/// Authenticate against a New API relay and save the selected key to config.
///
/// This is the interactive entry point wired into `codex login --newapi`.
pub async fn run_newapi_login(
    config_overrides: CliConfigOverrides,
    provided_base_url: Option<String>,
    use_access_token: bool,
) -> ! {
    let product = product_name();
    eprintln!("{product} - New API relay setup");
    let config = load_config_or_exit(config_overrides).await;

    let base_url = resolve_base_url(provided_base_url);
    if base_url.is_empty() {
        eprintln!("Error: New API base URL cannot be empty.");
        { pause_on_exit(); std::process::exit(1); }
    }
    let http = match build_http_client(&base_url) {
        Ok(client) => client,
        Err(err) => {
            eprintln!("Error: {err}");
            { pause_on_exit(); std::process::exit(1); }
        }
    };
    let client = NewApiClient::new(http, base_url.clone());

    // Direct-token mode: the user already has a New API key and just wants to
    // point Codex at it. No admin login required.
    if use_access_token {
        let token = read_line("New API Access Token (sk-...): ");
        if token.trim().is_empty() {
            eprintln!("Error: access token cannot be empty.");
            { pause_on_exit(); std::process::exit(1); }
        }
        let model = read_line("Model ID (default: gpt-4o): ");
        let model = if model.trim().is_empty() {
            DEFAULT_MODEL.to_string()
        } else {
            model.trim().to_string()
        };
        write_newapi_config(&config, &base_url, token.trim(), &model).await;
        std::process::exit(0);
    }

    let username = read_line("New API username: ");
    if username.trim().is_empty() {
        eprintln!("Error: username cannot be empty.");
        { pause_on_exit(); std::process::exit(1); }
    }
    let password = read_line("New API password: ");
    if password.is_empty() {
        eprintln!("Error: password cannot be empty.");
        { pause_on_exit(); std::process::exit(1); }
    }

    let (user_token, user_id) = match client.login(&username, &password).await {
        Ok(LoginResult::Direct { token, user_id }) => (token, user_id),
        Ok(LoginResult::Need2fa { flow_token }) => {
            eprintln!("This account requires two-factor authentication.");
            let code = read_line("Two-factor code: ");
            if code.trim().is_empty() {
                eprintln!("Error: two-factor code cannot be empty.");
                { pause_on_exit(); std::process::exit(1); }
            }
            match client.login_2fa(&flow_token, code.trim()).await {
                Ok((token, user_id)) => (token, user_id),
                Err(err) => {
                    eprintln!("Error verifying two-factor code: {err}");
                    { pause_on_exit(); std::process::exit(1); }
                }
            }
        }
        Err(err) => {
            eprintln!("Error logging in: {err}");
            { pause_on_exit(); std::process::exit(1); }
        }
    };

    let tokens = match client.list_tokens(&user_token, user_id).await {
        Ok(tokens) => tokens,
        Err(err) => {
            eprintln!("Error listing tokens: {err}");
            { pause_on_exit(); std::process::exit(1); }
        }
    };

    if tokens.is_empty() {
        eprintln!("No existing API keys. Creating a new one...");
    } else {
        eprintln!("Available API keys:");
        for (index, token) in tokens.iter().enumerate() {
            let masked = token
                .key
                .as_deref()
                .map_or_else(|| "<masked>".to_string(), mask_key);
            let quota = if token.unlimited_quota.unwrap_or(false) {
                "unlimited".to_string()
            } else {
                format_quota(token.remain_quota)
            };
            eprintln!(
                "  [{index}] {}  key: {masked}  quota: {quota}  expires: {}",
                token.name,
                format_expiry(token.expired_time),
            );
        }
    }

    let selection =
        read_line("Select a key number, or press Enter / type 'n' to create a new one: ");
    let token_key = select_or_create_key(&client, &user_token, user_id, &tokens, selection.trim()).await;

    let model = read_line("Model ID (default: gpt-4o): ");
    let model = if model.trim().is_empty() {
        DEFAULT_MODEL.to_string()
    } else {
        model.trim().to_string()
    };

    write_newapi_config(&config, &base_url, &token_key, &model).await;
    std::process::exit(0)
}

/// Resolves the relay base URL from CLI input, normalizing away a trailing
/// slash and any accidental `/v1` suffix.
/// Returns the relay base URL to pre-fill in the login prompt, preferring a
/// build-time `NEWAPI_BASE_URL` override and falling back to the branded default.
fn default_base_url() -> &'static str {
    DEFAULT_BASE_URL
        .filter(|url| !url.trim().is_empty())
        .unwrap_or(FALLBACK_BASE_URL)
}

/// Returns the product display name, preferring a build-time
/// `NEWAPI_PRODUCT_NAME` override and falling back to the branded default.
fn product_name() -> &'static str {
    option_env!("NEWAPI_PRODUCT_NAME")
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(FALLBACK_PRODUCT_NAME)
}

/// Resolves the relay base URL from CLI input, normalizing away a trailing
/// slash and any accidental `/v1` suffix.
fn resolve_base_url(provided: Option<String>) -> String {
    match provided {
        Some(raw) if !raw.trim().is_empty() => normalize_base_url(&raw),
        _ => {
            let default = default_base_url();
            let input = read_line(&format!("New API base URL (default {default}, omit /v1): "));
            if input.trim().is_empty() {
                normalize_base_url(default)
            } else {
                normalize_base_url(&input)
            }
        }
    }
}

/// Strips a trailing slash and an optional `/v1` suffix so the provider base
/// URL composed later is always `<base>/v1`.
fn normalize_base_url(raw: &str) -> String {
    let mut url = raw.trim().trim_end_matches('/').to_string();
    if let Some(stripped) = url.strip_suffix("/v1") {
        url = stripped.trim_end_matches('/').to_string();
    }
    url
}

/// Builds a proxy-aware HTTP client for the New API relay.
fn build_http_client(base_url: &str) -> Result<HttpClient, String> {
    let factory = HttpClientFactory::new(OutboundProxyPolicy::RespectSystemProxy);
    factory
        .build_client(base_url, ClientRouteClass::Auth)
        .map_err(|err| format!("Failed to build HTTP client: {err}"))
}

/// Waits for a key press before closing the console when running
/// interactively, so a double-clicked Windows exe does not vanish on error.
fn pause_on_exit() {
    use std::io::IsTerminal;
    if !io::stdin().is_terminal() {
        return;
    }
    eprintln!();
    eprint!("Press Enter to close...");
    let _ = io::stderr().flush();
    let mut line = String::new();
    let _ = io::stdin().read_line(&mut line);
}

/// Reads one trimmed line from stdin, printing the prompt to stderr.
fn read_line(prompt: &str) -> String {
    eprint!("{prompt}");
    let _ = io::stderr().flush();
    let mut line = String::new();
    if io::stdin().read_line(&mut line).is_err() {
        eprintln!();
        eprintln!("Error: failed to read input.");
        { pause_on_exit(); std::process::exit(1); }
    }
    line.trim().to_string()
}

/// Masks a key for display, keeping only a short prefix and suffix.
fn mask_key(key: &str) -> String {
    if key.len() <= 12 {
        return "***".to_string();
    }
    let prefix = &key[..6];
    let suffix = &key[key.len() - 4..];
    format!("{prefix}***{suffix}")
}

/// Formats a New API quota value (in units) as a dollar amount.
fn format_quota(quota: Option<i64>) -> String {
    match quota {
        Some(value) => format!("${:.2}", value as f64 / QUOTA_PER_DOLLAR as f64),
        None => "unknown".to_string(),
    }
}

/// Formats an expiry timestamp, with `-1` meaning never expires.
fn format_expiry(expired_time: Option<i64>) -> String {
    match expired_time {
        Some(time) if time < 0 => "never".to_string(),
        Some(time) => match chrono::DateTime::from_timestamp(time, 0) {
            Some(date) => date.format("%Y-%m-%d").to_string(),
            None => time.to_string(),
        },
        None => "unknown".to_string(),
    }
}

/// Lets the user pick an existing key or create a fresh one, returning the
/// usable API key string.
async fn select_or_create_key(
    client: &NewApiClient,
    user_token: &str,
    user_id: u64,
    tokens: &[NewApiToken],
    selection: &str,
) -> String {
    let selection = selection.trim();
    let should_create = selection.is_empty() || selection.eq_ignore_ascii_case("n");

    if should_create {
        return create_token_flow(client, user_token, user_id).await;
    }

    match selection.parse::<usize>() {
        Ok(index) if index < tokens.len() => {
            match client
                .get_token_key(user_token, user_id, tokens[index].id)
                .await
            {
                Ok(key) => key,
                Err(err) => {
                    eprintln!("Error retrieving selected key: {err}");
                    eprintln!("Creating a new key instead.");
                    create_token_flow(client, user_token, user_id).await
                }
            }
        }
        _ => {
            eprintln!("Invalid selection; creating a new key instead.");
            create_token_flow(client, user_token, user_id).await
        }
    }
}

/// Collects token details from the user and creates a new API key.
///
/// This is the interactive companion to `NewApiClient::create_token`. It is
/// synchronous because it only blocks on stdin, not on the async HTTP call.
async fn create_token_flow(client: &NewApiClient, user_token: &str, user_id: u64) -> String {
    let name = read_line("New key name (default: codex): ");
    let name = if name.trim().is_empty() {
        "codex".to_string()
    } else {
        name.trim().to_string()
    };

    let unlimited = read_line("Unlimited quota? (y/N): ").eq_ignore_ascii_case("y");
    let remain_quota = if unlimited {
        0
    } else {
        let dollars = read_line("Quota in dollars (default: 10): ");
        let dollars: i64 = dollars.trim().parse().unwrap_or(10);
        dollars.saturating_mul(QUOTA_PER_DOLLAR)
    };

    let expired_days = read_line("Expire in days (enter -1 for never, default: 90): ");
    let expired_time = {
        let days: i64 = expired_days.trim().parse().unwrap_or(90);
        if days < 0 {
            -1
        } else {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_secs() as i64)
                .unwrap_or(0);
            now.saturating_add(days.saturating_mul(86_400))
        }
    };

    let request = CreateTokenRequest {
        name,
        expired_time,
        remain_quota,
        unlimited_quota: unlimited,
        model_limits_enabled: false,
        model_limits: Vec::new(),
        allow_ips: String::new(),
        group: String::new(),
    };

    match client.create_token(user_token, user_id, &request).await {
        Ok(key) => {
            eprintln!("Created new API key: {}", mask_key(&key));
            key
        }
        Err(err) => {
            eprintln!("Error creating token: {err}");
            { pause_on_exit(); std::process::exit(1); }
        }
    }
}

/// Persists the New API provider and active provider selection to config.
async fn write_newapi_config(config: &Config, base_url: &str, token: &str, model: &str) {
    let edits = ConfigEditsBuilder::for_config(config)
        .with_edits(build_newapi_provider_edits(base_url, token, model));
    if let Err(err) = edits.apply().await {
        eprintln!("Error writing config: {err}");
        { pause_on_exit(); std::process::exit(1); }
    }
    eprintln!("New API configured successfully (provider: {PROVIDER_ID}).");
    eprintln!("Run `codex` to start using it.");
}

/// Builds the config edits for the active New API provider.
fn build_newapi_provider_edits(base_url: &str, token: &str, model: &str) -> Vec<ConfigEdit> {
    vec![
        ConfigEdit::SetPath {
            segments: vec![
                "model_providers".to_string(),
                PROVIDER_ID.to_string(),
                "name".to_string(),
            ],
            value: value(PROVIDER_NAME.to_string()),
        },
        ConfigEdit::SetPath {
            segments: vec![
                "model_providers".to_string(),
                PROVIDER_ID.to_string(),
                "base_url".to_string(),
            ],
            value: value(format!("{base_url}/v1")),
        },
        ConfigEdit::SetPath {
            segments: vec![
                "model_providers".to_string(),
                PROVIDER_ID.to_string(),
                "wire_api".to_string(),
            ],
            value: value("responses".to_string()),
        },
        ConfigEdit::SetPath {
            segments: vec![
                "model_providers".to_string(),
                PROVIDER_ID.to_string(),
                "experimental_bearer_token".to_string(),
            ],
            value: value(token.to_string()),
        },
        ConfigEdit::SetPath {
            segments: vec!["model".to_string()],
            value: value(model.to_string()),
        },
        ConfigEdit::SetPath {
            segments: vec!["model_provider".to_string()],
            value: value(PROVIDER_ID.to_string()),
        },
    ]
}

/// Minimal New API client for the endpoints this login flow uses.
struct NewApiClient {
    http: HttpClient,
    base_url: String,
}

impl NewApiClient {
    fn new(http: HttpClient, base_url: String) -> Self {
        Self { http, base_url }
    }

    /// POSTs `/api/user/login` and returns either direct credentials or a 2FA
    /// flow token when the account requires two-factor authentication.
    async fn login(&self, username: &str, password: &str) -> Result<LoginResult, String> {
        let base_url = &self.base_url;
        let url = format!("{base_url}/api/user/login");
        let response = self
            .http
            .post(url.as_str())
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({ "username": username, "password": password }))
            .send()
            .await
            .map_err(|err| format!("Login request failed ({url}): {err}"))?;
        let data: LoginData = parse_envelope(response, "login").await?;
        if data.require_2fa.unwrap_or(false) {
            let flow_token = data
                .flow_token
                .ok_or_else(|| "Login response missing 2FA flow token".to_string())?;
            return Ok(LoginResult::Need2fa { flow_token });
        }
        let token = data
            .token
            .ok_or_else(|| "Login response missing access token".to_string())?;
        let user_id = data
            .user
            .map(|user| user.id)
            .or(data.id)
            .ok_or_else(|| "Login response missing user id".to_string())?;
        Ok(LoginResult::Direct { token, user_id })
    }

    /// Completes a 2FA login with the flow token and the authenticator code.
    async fn login_2fa(&self, flow_token: &str, code: &str) -> Result<(String, u64), String> {
        let base_url = &self.base_url;
        let url = format!("{base_url}/api/user/login/2fa");
        let response = self
            .http
            .post(url.as_str())
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({ "flow_token": flow_token, "code": code }))
            .send()
            .await
            .map_err(|err| format!("2FA login request failed ({url}): {err}"))?;
        let data: LoginData = parse_envelope(response, "2FA login").await?;
        let token = data
            .token
            .ok_or_else(|| "2FA login response missing access token".to_string())?;
        let user_id = data
            .user
            .map(|user| user.id)
            .or(data.id)
            .ok_or_else(|| "2FA login response missing user id".to_string())?;
        Ok((token, user_id))
    }

    /// GETs the token list for the authenticated user.
    async fn list_tokens(&self, user_token: &str, user_id: u64) -> Result<Vec<NewApiToken>, String> {
        let base_url = &self.base_url;
        let url = format!("{base_url}/api/token/?p=1&size=100");
        let response = self
            .http
            .get(url.as_str())
            .bearer_auth(user_token)
            .header("New-Api-User", user_id.to_string())
            .send()
            .await
            .map_err(|err| format!("List token request failed ({url}): {err}"))?;
        let data: TokenListData = parse_envelope(response, "list tokens").await?;
        Ok(data.into_items())
    }

    /// POSTs a new token, then resolves the just-created token's id and fetches
    /// its full key. New API's `AddToken` returns no `data`, so the key must be
    /// retrieved via `/api/token/{id}/key` afterwards.
    async fn create_token(
        &self,
        user_token: &str,
        user_id: u64,
        request: &CreateTokenRequest,
    ) -> Result<String, String> {
        let base_url = &self.base_url;
        let create_url = format!("{base_url}/api/token/");
        let response = self
            .http
            .post(create_url.as_str())
            .bearer_auth(user_token)
            .header("New-Api-User", user_id.to_string())
            .header("Content-Type", "application/json")
            .json(request)
            .send()
            .await
            .map_err(|err| format!("Create token request failed ({create_url}): {err}"))?;
        ensure_envelope_success(response, "create token").await?;

        let search_url = format!(
            "{base_url}/api/token/search?keyword={}&p=1&size=100",
            encode_url_component(&request.name)
        );
        let response = self
            .http
            .get(search_url.as_str())
            .bearer_auth(user_token)
            .header("New-Api-User", user_id.to_string())
            .send()
            .await
            .map_err(|err| format!("Search token request failed ({search_url}): {err}"))?;
        let data: TokenListData = parse_envelope(response, "search tokens").await?;
        let token = data
            .into_items()
            .into_iter()
            .filter(|token| token.name == request.name)
            .max_by_key(|token| token.id)
            .ok_or_else(|| format!("Created token '{}' not found in token list", request.name))?;

        self.get_token_key(user_token, user_id, token.id).await
    }

    /// POSTs `/api/token/{id}/key` and returns the full plaintext API key.
    async fn get_token_key(
        &self,
        user_token: &str,
        user_id: u64,
        token_id: u64,
    ) -> Result<String, String> {
        let base_url = &self.base_url;
        let url = format!("{base_url}/api/token/{token_id}/key");
        let response = self
            .http
            .post(url.as_str())
            .bearer_auth(user_token)
            .header("New-Api-User", user_id.to_string())
            .send()
            .await
            .map_err(|err| format!("Get token key request failed ({url}): {err}"))?;
        let data: TokenKeyData = parse_envelope(response, "get token key").await?;
        Ok(data.key)
    }
}

/// Reads and validates a New API envelope, returning the inner `data`.
async fn parse_envelope<T: for<'de> Deserialize<'de>>(
    response: codex_http_client::HttpResponse,
    context: &str,
) -> Result<T, String> {
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "{context} request failed (HTTP {}): {body}",
            status.as_u16()
        ));
    }
    let body = response
        .text()
        .await
        .map_err(|err| format!("Failed to read {context} response: {err}"))?;
    let envelope: serde_json::Value = serde_json::from_str(&body)
        .map_err(|err| format!("Failed to parse {context} response: {err}"))?;
    let success = envelope
        .get("success")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if !success {
        let message = envelope
            .get("message")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .filter(|message| !message.is_empty())
            .unwrap_or_else(|| format!("{context} request failed"));
        return Err(message);
    }
    let data = envelope
        .get("data")
        .ok_or_else(|| format!("{context} response missing data"))?;
    serde_json::from_value(data.clone())
        .map_err(|err| format!("Failed to parse {context} response data: {err}"))
}

/// Validates that a New API envelope reports success, allowing responses that
/// carry no `data` (such as `CreateToken`).
async fn ensure_envelope_success(
    response: codex_http_client::HttpResponse,
    context: &str,
) -> Result<(), String> {
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "{context} request failed (HTTP {}): {body}",
            status.as_u16()
        ));
    }
    let body = response
        .text()
        .await
        .map_err(|err| format!("Failed to read {context} response: {err}"))?;
    let envelope: serde_json::Value = serde_json::from_str(&body)
        .map_err(|err| format!("Failed to parse {context} response: {err}"))?;
    let success = envelope
        .get("success")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if !success {
        let message = envelope
            .get("message")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .filter(|message| !message.is_empty())
            .unwrap_or_else(|| format!("{context} request failed"));
        return Err(message);
    }
    Ok(())
}

/// Percent-encodes a string for use as a URL query component.
fn encode_url_component(raw: &str) -> String {
    let mut out = String::new();
    for byte in raw.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{byte:02X}"));
            }
        }
    }
    out
}


#[derive(Debug, Deserialize)]
struct LoginData {
    #[serde(alias = "access_token")]
    token: Option<String>,
    #[serde(default)]
    user: Option<LoginUser>,
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    require_2fa: Option<bool>,
    #[serde(default)]
    flow_token: Option<String>,
}

#[derive(Debug)]
enum LoginResult {
    Direct { token: String, user_id: u64 },
    Need2fa { flow_token: String },
}

#[derive(Debug, Deserialize)]
struct LoginUser {
    id: u64,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TokenListData {
    /// New API returns the list under `data.items` in some versions.
    Items { items: Vec<NewApiToken> },
    /// New API returns the list directly as `data: [...]` in others.
    Direct(Vec<NewApiToken>),
}

impl TokenListData {
    fn into_items(self) -> Vec<NewApiToken> {
        match self {
            Self::Items { items } => items,
            Self::Direct(items) => items,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct NewApiToken {
    #[allow(dead_code)]
    id: u64,
    name: String,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    status: Option<i64>,
    #[serde(default)]
    remain_quota: Option<i64>,
    #[serde(default)]
    unlimited_quota: Option<bool>,
    #[serde(default)]
    expired_time: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct TokenKeyData {
    key: String,
}

#[derive(Debug, Serialize)]
struct CreateTokenRequest {
    name: String,
    expired_time: i64,
    remain_quota: i64,
    unlimited_quota: bool,
    model_limits_enabled: bool,
    model_limits: Vec<String>,
    allow_ips: String,
    group: String,
}

#[cfg(test)]
#[path = "newapi_login_tests.rs"]
mod tests;



