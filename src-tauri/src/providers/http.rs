//! The one way a provider's usage endpoint is asked, so a 401, a 429 and a
//! 5xx mean the same thing for every provider.

use std::sync::OnceLock;
use std::time::Duration;

use crate::model::Reason;

pub enum Outcome {
    Ok(serde_json::Value),
    /// 401/403: the stored token is dead. Worth another route, not a retry.
    NeedsFreshCredentials,
    Failed(Reason),
}

/// How many extra attempts a stumbling connection gets.
const RETRY_LIMIT: u32 = 2;

pub fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("an HTTP client with default settings")
    })
}

pub async fn get_json(url: &str, headers: &[(&str, &str)]) -> Outcome {
    for attempt in 0..=RETRY_LIMIT {
        let mut request = client().get(url).header("Accept", "application/json");
        for (name, value) in headers {
            request = request.header(*name, *value);
        }

        match request.send().await {
            Ok(response) => {
                return match response.status().as_u16() {
                    200 => match response.json::<serde_json::Value>().await {
                        Ok(value) if value.is_object() => Outcome::Ok(value),
                        _ => Outcome::Failed(Reason::UnreadableReply),
                    },
                    401 | 403 => Outcome::NeedsFreshCredentials,
                    429 => Outcome::Failed(Reason::RateLimited),
                    _ => Outcome::Failed(Reason::ServerError),
                };
            }
            Err(error) => {
                // A proxy or VPN dropping a connection usually clears on a
                // second try; anything else will fail the same way again.
                let worth_retrying = error.is_connect() || error.is_timeout() || error.is_request();
                if attempt < RETRY_LIMIT && worth_retrying {
                    tokio::time::sleep(Duration::from_millis(600 * (attempt as u64 + 1))).await;
                    continue;
                }
                return Outcome::Failed(Reason::Unreachable);
            }
        }
    }
    Outcome::Failed(Reason::Unreachable)
}
