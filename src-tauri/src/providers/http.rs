//! The one way a provider's usage endpoint is asked, so a 401, a 429 and a
//! 5xx mean the same thing for every provider.

use std::sync::OnceLock;
use std::time::Duration;

use crate::model::{now_ms, Reason};

pub enum Outcome {
    Ok(serde_json::Value),
    /// 401/403: the stored token is dead. Worth another route, not a retry.
    NeedsFreshCredentials,
    /// 429: asked too often. How long the server said to wait, in ms, if it said.
    RateLimited(Option<i64>),
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
                    429 => Outcome::RateLimited(retry_after(response.headers(), now_ms())),
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

/// `Retry-After` in ms from `now`: a number of seconds, or a date.
fn retry_after(headers: &reqwest::header::HeaderMap, now: i64) -> Option<i64> {
    let value = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?.trim();
    let ms = match value.parse::<i64>() {
        Ok(seconds) => seconds.checked_mul(1000)?,
        Err(_) => chrono::DateTime::parse_from_rfc2822(value).ok()?.timestamp_millis() - now,
    };
    (ms > 0).then_some(ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};

    fn saying(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_str(value).unwrap());
        headers
    }

    #[test]
    fn retry_after_in_seconds_or_as_a_date() {
        assert_eq!(retry_after(&saying("120"), 0), Some(120_000));
        // Wed, 21 Oct 2015 07:28:00 GMT, asked a minute before.
        assert_eq!(retry_after(&saying("Wed, 21 Oct 2015 07:28:00 GMT"), 1_445_412_420_000), Some(60_000));
        // Past, nothing, or nonsense: not said.
        assert_eq!(retry_after(&saying("0"), 0), None);
        assert_eq!(retry_after(&saying("Wed, 21 Oct 2015 07:28:00 GMT"), 1_445_412_540_000), None);
        assert_eq!(retry_after(&saying("soon"), 0), None);
        assert_eq!(retry_after(&HeaderMap::new(), 0), None);
    }

    #[test]
    fn a_429_brings_its_wait_along() {
        use std::io::{Read, Write};
        // A server on this PC that refuses once, as Anthropic's does.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/usage", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = stream.read(&mut [0; 4096]);
            let reply = "HTTP/1.1 429 Too Many Requests
Retry-After: 90
Content-Length: 0
Connection: close

";
            stream.write_all(reply.as_bytes()).unwrap();
        });
        let outcome = tauri::async_runtime::block_on(get_json(&url, &[]));
        server.join().unwrap();
        assert!(matches!(outcome, Outcome::RateLimited(Some(90_000))));
    }
}
