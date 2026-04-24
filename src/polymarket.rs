use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use reqwest::{Client, StatusCode};
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::time::sleep;

use crate::config::AppConfig;
use crate::domain::{
    ClosedPosition, LeaderboardCategory, LeaderboardEntry, LeaderboardOrderBy,
    LeaderboardTimePeriod, Position, WalletActivity,
};

#[derive(Clone)]
pub struct PolymarketClient {
    http: Client,
    data_api_base_url: String,
    clob_api_base_url: String,
    _gamma_api_base_url: String,
    retry_attempts: usize,
    retry_backoff_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ResolvedWallet {
    pub wallet: String,
    pub label: Option<String>,
    pub username: Option<String>,
}

impl PolymarketClient {
    pub fn new(config: &AppConfig) -> Result<Self> {
        let http = Client::builder()
            .user_agent("mochi_gain_hunter/0.1.0")
            .timeout(Duration::from_secs(config.http.request_timeout_secs))
            .connect_timeout(Duration::from_secs(config.http.connect_timeout_secs))
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(30))
            .build()?;

        Ok(Self {
            http,
            data_api_base_url: config.data_api_base_url.trim_end_matches('/').to_owned(),
            clob_api_base_url: config.clob_api_base_url.trim_end_matches('/').to_owned(),
            _gamma_api_base_url: config.gamma_api_base_url.trim_end_matches('/').to_owned(),
            retry_attempts: config.http.retry_attempts,
            retry_backoff_ms: config.http.retry_backoff_ms,
        })
    }

    pub async fn leaderboard(
        &self,
        category: LeaderboardCategory,
        time_period: LeaderboardTimePeriod,
        order_by: LeaderboardOrderBy,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<LeaderboardEntry>> {
        let params = vec![
            ("category", category.as_api_str().to_owned()),
            ("timePeriod", time_period.as_api_str().to_owned()),
            ("orderBy", order_by.as_api_str().to_owned()),
            ("limit", limit.to_string()),
            ("offset", offset.to_string()),
        ];

        self.data_get("/v1/leaderboard", &params).await
    }

    pub async fn user_activity(
        &self,
        wallet: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<WalletActivity>> {
        let params = vec![
            ("user", wallet.to_owned()),
            ("limit", limit.to_string()),
            ("offset", offset.to_string()),
            ("sortBy", "TIMESTAMP".to_owned()),
            ("sortDirection", "DESC".to_owned()),
        ];

        self.data_get("/activity", &params).await
    }

    pub async fn current_positions(&self, wallet: &str, limit: usize) -> Result<Vec<Position>> {
        let params = vec![
            ("user", wallet.to_owned()),
            ("limit", limit.to_string()),
            ("sizeThreshold", "0".to_owned()),
            ("sortBy", "CASHPNL".to_owned()),
            ("sortDirection", "DESC".to_owned()),
        ];

        self.data_get("/positions", &params).await
    }

    pub async fn closed_positions(
        &self,
        wallet: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<ClosedPosition>> {
        let params = vec![
            ("user", wallet.to_owned()),
            ("limit", limit.to_string()),
            ("offset", offset.to_string()),
            ("sortBy", "REALIZEDPNL".to_owned()),
        ];

        self.data_get("/closed-positions", &params).await
    }

    pub async fn midpoint_price(&self, token_id: &str) -> Result<Option<f64>> {
        let params = vec![("token_id", token_id.to_owned())];
        let url = format!("{}/midpoint", self.clob_api_base_url);
        let response = self
            .send_with_retry(
                || self.http.get(url.clone()).query(&params),
                &format!("fetch midpoint for token {token_id}"),
            )
            .await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        let response = response.error_for_status()?;
        let payload: Value = response.json().await?;

        let Some(price_value) = payload
            .get("mid_price")
            .or_else(|| payload.get("midpoint"))
            .or_else(|| payload.get("midPrice"))
        else {
            return Ok(None);
        };

        let price = match price_value {
            Value::String(value) => value
                .parse::<f64>()
                .with_context(|| format!("failed to parse midpoint for token {token_id}"))?,
            Value::Number(value) => value
                .as_f64()
                .ok_or_else(|| anyhow!("midpoint number is not representable as f64"))?,
            _ => return Ok(None),
        };

        Ok(Some(price))
    }

    pub async fn resolve_wallet_input(&self, input: &str) -> Result<ResolvedWallet> {
        let trimmed = input.trim();
        if let Some(wallet) = extract_wallet_address(trimmed) {
            validate_wallet_address(&wallet)?;
            return Ok(ResolvedWallet {
                wallet: wallet.to_ascii_lowercase(),
                label: None,
                username: None,
            });
        }

        let handle = extract_profile_handle(trimmed).ok_or_else(|| {
            anyhow!("could not find a 0x wallet address or Polymarket profile handle")
        })?;
        self.resolve_profile_handle(&handle).await
    }

    async fn resolve_profile_handle(&self, handle: &str) -> Result<ResolvedWallet> {
        let normalized_handle = handle.trim().trim_start_matches('@');
        let profile_url = format!("https://polymarket.com/@{normalized_handle}");
        let html = self
            .send_with_retry(
                || self.http.get(profile_url.clone()),
                &format!("resolve profile handle @{normalized_handle}"),
            )
            .await?
            .error_for_status()?
            .text()
            .await?;

        let wallet = extract_json_string(&html, "\"proxyAddress\":\"")
            .or_else(|| extract_json_string(&html, "\"proxyWallet\":\""))
            .or_else(|| extract_wallet_address(&html))
            .ok_or_else(|| anyhow!("could not resolve wallet from profile page"))?;
        validate_wallet_address(&wallet)?;

        let pseudonym = extract_last_json_string(&html, "\"pseudonym\":\"");
        let username = extract_last_json_string(&html, "\"username\":\"")
            .or_else(|| Some(normalized_handle.to_owned()));
        let name = extract_last_json_string(&html, "\"name\":\"");

        Ok(ResolvedWallet {
            wallet: wallet.to_ascii_lowercase(),
            label: pseudonym.clone().or(name).or(username.clone()),
            username,
        })
    }

    async fn data_get<T>(&self, path: &str, params: &[(&str, String)]) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let url = format!("{}{}", self.data_api_base_url, path);
        let response = self
            .send_with_retry(
                || self.http.get(url.clone()).query(params),
                &format!("fetch data api path {path}"),
            )
            .await?;
        let response = response.error_for_status()?;
        Ok(response.json::<T>().await?)
    }

    async fn send_with_retry<F>(&self, build_request: F, context: &str) -> Result<reqwest::Response>
    where
        F: Fn() -> reqwest::RequestBuilder,
    {
        let max_attempts = self.retry_attempts().max(1);
        let mut last_error = None;

        for attempt in 0..max_attempts {
            match build_request().send().await {
                Ok(response) => {
                    if should_retry_status(response.status()) && attempt + 1 < max_attempts {
                        sleep(self.retry_delay(attempt)).await;
                        continue;
                    }

                    return Ok(response);
                }
                Err(error) => {
                    let retryable = is_retryable_request_error(&error);
                    last_error = Some(error);
                    if retryable && attempt + 1 < max_attempts {
                        sleep(self.retry_delay(attempt)).await;
                        continue;
                    }
                    break;
                }
            }
        }

        Err(last_error
            .map(anyhow::Error::from)
            .unwrap_or_else(|| anyhow!("request failed without an error")))
        .with_context(|| format!("{context} after {} attempt(s)", max_attempts))
    }

    fn retry_attempts(&self) -> usize {
        self.retry_attempts
    }

    fn retry_delay(&self, attempt: usize) -> Duration {
        let base_ms = self.retry_backoff_ms.max(1);
        let multiplier = 1_u64 << attempt.min(4);
        Duration::from_millis(base_ms.saturating_mul(multiplier))
    }
}

fn should_retry_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::REQUEST_TIMEOUT
            | StatusCode::TOO_MANY_REQUESTS
            | StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT
    ) || status.is_server_error()
}

fn is_retryable_request_error(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect() || error.is_request()
}

pub fn validate_wallet_address(wallet: &str) -> Result<()> {
    if wallet.len() != 42 || !wallet.starts_with("0x") {
        return Err(anyhow!("wallet must be a 0x-prefixed 40-hex address"));
    }

    if !wallet.bytes().skip(2).all(|byte| byte.is_ascii_hexdigit()) {
        return Err(anyhow!("wallet contains non-hex characters"));
    }

    Ok(())
}

pub fn extract_profile_handle(input: &str) -> Option<String> {
    let trimmed = input.trim();

    if let Some(at_index) = trimmed.find("polymarket.com/@") {
        let start = at_index + "polymarket.com/@".len();
        return Some(trimmed[start..].split(['/', '?', '#']).next()?.to_owned());
    }

    if let Some(profile_index) = trimmed.find("polymarket.com/profile/%40") {
        let start = profile_index + "polymarket.com/profile/%40".len();
        return Some(trimmed[start..].split(['/', '?', '#']).next()?.to_owned());
    }

    if let Some(handle) = trimmed.strip_prefix('@')
        && !handle.is_empty()
    {
        return Some(handle.to_owned());
    }

    if !trimmed.is_empty()
        && !trimmed.contains(char::is_whitespace)
        && !trimmed.contains('/')
        && !trimmed.starts_with("http")
    {
        return Some(trimmed.to_owned());
    }

    None
}

fn extract_wallet_address(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    for index in 0..bytes.len().saturating_sub(41) {
        if bytes[index] == b'0' && bytes.get(index + 1) == Some(&b'x') {
            let candidate = &input[index..index + 42];
            if candidate
                .bytes()
                .skip(2)
                .all(|byte| byte.is_ascii_hexdigit())
            {
                return Some(candidate.to_owned());
            }
        }
    }

    None
}

fn extract_json_string(input: &str, marker: &str) -> Option<String> {
    let start = input.find(marker)? + marker.len();
    let rest = &input[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

fn extract_last_json_string(input: &str, marker: &str) -> Option<String> {
    let start = input.rfind(marker)? + marker.len();
    let rest = &input[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}
