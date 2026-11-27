use crate::{
    CanyonError, Result,
    cache::{CacheEntry, CacheStats, HttpCache, default_cache_path},
    timestamp,
};
use chrono::Utc;
use reqwest::{
    Client, StatusCode,
    header::{self, HeaderMap, HeaderValue},
};
use std::{
    env,
    sync::OnceLock,
    time::{Duration, SystemTime},
};
use tokio::{
    sync::Mutex,
    time::{Instant, sleep, sleep_until},
};
use url::Url;

const USER_AGENT: &str = "canyon-mcp/0.1 (+https://github.com/jb381/canyon-mcp)";
const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug)]
pub struct FetchResult {
    pub body: String,
    pub url: Url,
    pub fetched_at: String,
    pub from_cache: bool,
    pub stale: bool,
}

fn cached_result(url: Url, entry: CacheEntry, stale: bool) -> FetchResult {
    FetchResult {
        body: entry.body,
        url,
        fetched_at: entry.fetched_at,
        from_cache: true,
        stale,
    }
}

pub fn assert_allowed_canyon_url(value: &str) -> Result<Url> {
    let url = Url::parse(value)
        .map_err(|_| CanyonError::new("URL_NOT_ALLOWED", format!("Invalid Canyon URL: {value}")))?;
    allowed_url(url)
}

fn allowed_url(mut url: Url) -> Result<Url> {
    if url.scheme() != "https"
        || url.host_str() != Some("www.canyon.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(CanyonError::new(
            "URL_NOT_ALLOWED",
            format!("Refusing non-Canyon URL: {url}"),
        ));
    }
    if url.path().starts_with("/on/demandware.store/") {
        return Err(CanyonError::new(
            "ROBOTS_DISALLOWED",
            format!(
                "Refusing Canyon endpoint disallowed by robots.txt: {}",
                url.path()
            ),
        ));
    }
    url.set_fragment(None);
    Ok(url)
}

pub struct CanyonHttpClient {
    cache: HttpCache,
    client: OnceLock<std::result::Result<Client, reqwest::Error>>,
    timeout: Duration,
    headers: HeaderMap,
    interval: Duration,
    next_request: Mutex<Instant>,
    #[cfg(test)]
    transport: Option<TestTransport>,
}

#[cfg(test)]
type TestTransport = Box<
    dyn Fn(
            Url,
            HeaderMap,
        )
            -> std::pin::Pin<Box<dyn Future<Output = reqwest::Result<reqwest::Response>> + Send>>
        + Send
        + Sync,
>;

impl CanyonHttpClient {
    pub fn from_env() -> Result<Self> {
        let interval = env::var("CANYON_MIN_INTERVAL_MS")
            .ok()
            .and_then(|s| {
                let millis: f64 = if s.trim().is_empty() {
                    0.0
                } else {
                    s.trim().parse().ok()?
                };
                Duration::try_from_secs_f64(millis / 1000.0).ok()
            })
            .filter(|&interval| Instant::now().checked_add(interval).is_some())
            .unwrap_or(Duration::from_secs(2));
        let user_agent = env::var("CANYON_USER_AGENT").unwrap_or_else(|_| USER_AGENT.into());
        Self::new(
            HttpCache::open(default_cache_path())?,
            interval,
            Duration::from_secs(20),
            &user_agent,
        )
    }

    pub fn new(
        cache: HttpCache,
        interval: Duration,
        timeout: Duration,
        user_agent: &str,
    ) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ACCEPT,
            HeaderValue::from_static(
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            ),
        );
        headers.insert(
            header::ACCEPT_LANGUAGE,
            HeaderValue::from_static("en-US,en;q=0.8"),
        );
        headers.insert(
            header::USER_AGENT,
            user_agent.parse().map_err(|_| {
                CanyonError::new("NETWORK_ERROR", "Invalid Canyon User-Agent header")
            })?,
        );
        Ok(Self {
            cache,
            client: OnceLock::new(),
            timeout,
            headers,
            interval,
            next_request: Mutex::new(Instant::now()),
            #[cfg(test)]
            transport: None,
        })
    }

    pub fn cache_stats(&self) -> Result<CacheStats> {
        self.cache.stats()
    }

    pub async fn get(&self, url: &str, max_age: Duration) -> Result<FetchResult> {
        let url = assert_allowed_canyon_url(url)?;
        let key = url.as_str();
        let cached = self.cache.get(key)?;
        if !max_age.is_zero()
            && cached.as_ref().is_some_and(|entry| {
                chrono::DateTime::parse_from_rfc3339(&entry.fetched_at).is_ok_and(|fetched_at| {
                    Utc::now()
                        .signed_duration_since(fetched_at)
                        .num_milliseconds() as i128
                        <= max_age.as_millis() as i128
                })
            })
        {
            return Ok(cached_result(url, cached.unwrap(), false));
        }

        let mut headers = self.headers.clone();
        if let Some(entry) = &cached {
            set_validator(&mut headers, header::IF_NONE_MATCH, &entry.etag);
            set_validator(
                &mut headers,
                header::IF_MODIFIED_SINCE,
                &entry.last_modified,
            );
        }

        let mut last_error = CanyonError::new("NETWORK_ERROR", format!("Unable to fetch {url}"));
        for attempt in 0..3 {
            let backoff = Duration::from_secs(1 << attempt);
            match self.fetch_allowed(&url, headers.clone()).await {
                Ok((response, final_url)) => {
                    let status = response.status();
                    if status == StatusCode::NOT_MODIFIED
                        && let Some(mut entry) = cached
                    {
                        let fetched_at = timestamp();
                        if self.cache.touch(key, &entry, &fetched_at)? {
                            entry.fetched_at = fetched_at;
                        } else if let Some(latest) = self.cache.get(key)? {
                            entry = latest;
                        }
                        return Ok(cached_result(url, entry, false));
                    }
                    if status.is_success() {
                        let etag = response_header(&response, header::ETAG);
                        let last_modified = response_header(&response, header::LAST_MODIFIED);
                        match read_body(response).await {
                            Ok(body) => {
                                let fetched_at = timestamp();
                                let entry = CacheEntry {
                                    body,
                                    etag,
                                    last_modified,
                                    fetched_at,
                                };
                                self.cache.put(key, &entry)?;
                                return Ok(FetchResult {
                                    body: entry.body,
                                    url: final_url,
                                    fetched_at: entry.fetched_at,
                                    from_cache: false,
                                    stale: false,
                                });
                            }
                            Err(error) if error.code == "NETWORK_ERROR" => {}
                            Err(error) => return Err(error),
                        }
                    } else {
                        last_error = CanyonError::new(
                            "HTTP_ERROR",
                            format!("Canyon returned HTTP {} for {url}", status.as_u16()),
                        );
                        if status != StatusCode::TOO_MANY_REQUESTS && !status.is_server_error() {
                            return Err(last_error);
                        }
                        let delay = retry_after(response.headers())
                            .unwrap_or_default()
                            .max(backoff);
                        drop(response);
                        // Skip retries which would keep an interactive caller waiting indefinitely.
                        if attempt == 2 || delay > Duration::from_secs(60) {
                            break;
                        }
                        sleep(delay).await;
                        continue;
                    }
                }
                Err(error) if error.code == "NETWORK_ERROR" => {}
                Err(error) => return Err(error),
            }
            last_error = CanyonError::new("NETWORK_ERROR", format!("Unable to fetch {url}"));
            if attempt < 2 {
                sleep(backoff).await;
            }
        }

        cached
            .map(|entry| cached_result(url, entry, true))
            .ok_or(last_error)
    }

    async fn fetch_allowed(
        &self,
        initial: &Url,
        mut headers: HeaderMap,
    ) -> Result<(reqwest::Response, Url)> {
        let mut url = initial.clone();
        for redirects in 0..=5 {
            self.throttle().await;
            let response = self.send(url.clone(), headers.clone()).await?;
            if !matches!(response.status().as_u16(), 301 | 302 | 303 | 307 | 308) {
                return Ok((response, url));
            }
            let location = response
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .filter(|value| !value.is_empty())
                .and_then(|value| url.join(value).ok())
                .ok_or_else(|| {
                    CanyonError::new(
                        "INVALID_REDIRECT",
                        format!("Canyon redirect has no valid Location header: {url}"),
                    )
                })?;
            url = allowed_url(location)?;
            if redirects < 5 {
                headers.remove(header::IF_NONE_MATCH);
                headers.remove(header::IF_MODIFIED_SINCE);
            }
        }
        Err(CanyonError::new(
            "TOO_MANY_REDIRECTS",
            format!("Too many redirects for {initial}"),
        ))
    }

    async fn send(&self, url: Url, headers: HeaderMap) -> Result<reqwest::Response> {
        let network_error =
            |error: &reqwest::Error| CanyonError::new("NETWORK_ERROR", error.to_string());
        #[cfg(test)]
        if let Some(transport) = &self.transport {
            return transport(url, headers)
                .await
                .map_err(|error| network_error(&error));
        }
        let client = self
            .client
            .get_or_init(|| {
                Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .timeout(self.timeout)
                    .build()
            })
            .as_ref()
            .map_err(network_error)?;
        client
            .get(url)
            .headers(headers)
            .send()
            .await
            .map_err(|error| network_error(&error))
    }

    async fn throttle(&self) {
        let mut next = self.next_request.lock().await;
        if *next > Instant::now() {
            sleep_until(*next).await;
        }
        *next = Instant::now() + self.interval;
    }
}

fn set_validator(headers: &mut HeaderMap, name: header::HeaderName, value: &Option<String>) {
    if let Some(value) = value
        && !value.is_empty()
        && let Ok(value) = value.parse()
    {
        headers.insert(name, value);
    }
}

fn response_header(response: &reqwest::Response, name: header::HeaderName) -> Option<String> {
    response
        .headers()
        .get(name)?
        .to_str()
        .ok()
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    let value = headers.get(header::RETRY_AFTER)?.to_str().ok()?;
    if let Ok(seconds) = value.parse() {
        return Some(Duration::from_secs(seconds));
    }
    Some(
        httpdate::parse_http_date(value)
            .ok()?
            .duration_since(SystemTime::now())
            .unwrap_or_default(),
    )
}

async fn read_body(mut response: reqwest::Response) -> Result<String> {
    let size = response.content_length().unwrap_or(0);
    let too_large =
        || CanyonError::new("HTTP_ERROR", "Canyon response exceeds the cache size limit");
    if size > MAX_BODY_BYTES as u64 {
        return Err(too_large());
    }
    let mut bytes = Vec::with_capacity(size as usize);
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| CanyonError::new("NETWORK_ERROR", error.to_string()))?
    {
        if bytes.len() + chunk.len() > MAX_BODY_BYTES {
            return Err(too_large());
        }
        bytes.extend_from_slice(&chunk);
    }
    // Fetch's text() decodes as UTF-8, strips the BOM, and replaces invalid bytes.
    if bytes.starts_with(b"\xef\xbb\xbf") {
        bytes.drain(..3);
    }
    Ok(String::from_utf8(bytes)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex as StdMutex},
    };

    const PRODUCT: &str = "https://www.canyon.com/en-de/product";

    type Requests = Arc<StdMutex<Vec<(Url, HeaderMap, Instant)>>>;

    fn response(status: u16, headers: &[(&str, &str)], body: &'static str) -> reqwest::Response {
        let mut response = ::http::Response::builder().status(status);
        for (key, value) in headers {
            response = response.header(*key, *value);
        }
        response.body(body).unwrap().into()
    }

    fn mock(interval: Duration, responses: Vec<reqwest::Response>) -> (CanyonHttpClient, Requests) {
        let requests = Requests::default();
        let observed = requests.clone();
        let responses = StdMutex::new(VecDeque::from(responses));
        let mut client = CanyonHttpClient::new(
            HttpCache::open(":memory:").unwrap(),
            interval,
            Duration::from_secs(20),
            "test-agent",
        )
        .unwrap();
        client.transport = Some(Box::new(move |url, headers| {
            observed
                .lock()
                .unwrap()
                .push((url, headers, Instant::now()));
            let response = responses
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected request");
            Box::pin(async move { Ok(response) })
        }));
        (client, requests)
    }

    fn seed(client: &CanyonHttpClient, fetched_at: &str) {
        client
            .cache
            .put(
                PRODUCT,
                &CacheEntry {
                    body: "cached".into(),
                    etag: Some("\"v1\"".into()),
                    last_modified: Some("Mon, 01 Jan 2024 00:00:00 GMT".into()),
                    fetched_at: fetched_at.into(),
                },
            )
            .unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn serves_fresh_cache_and_refresh_revalidates_with_both_validators() {
        let (client, requests) = mock(Duration::ZERO, vec![response(304, &[], "")]);
        seed(&client, &timestamp());
        let fresh = client.get(PRODUCT, Duration::from_secs(60)).await.unwrap();
        assert_eq!(fresh.body, "cached");
        assert!(fresh.from_cache && !fresh.stale);
        assert!(requests.lock().unwrap().is_empty());

        let revalidated = client.get(PRODUCT, Duration::ZERO).await.unwrap();
        assert_eq!(revalidated.body, "cached");
        assert!(revalidated.from_cache && !revalidated.stale);
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].1[header::IF_NONE_MATCH], "\"v1\"");
        assert_eq!(
            requests[0].1[header::IF_MODIFIED_SINCE],
            "Mon, 01 Jan 2024 00:00:00 GMT"
        );
        assert_eq!(
            client.cache.get(PRODUCT).unwrap().unwrap().fetched_at,
            revalidated.fetched_at
        );
    }

    #[tokio::test(start_paused = true)]
    async fn validates_each_redirect_and_strips_validators_after_the_first_hop() {
        let redirects = vec![
            response(302, &[("location", "/en-de/new-product#top")], ""),
            response(200, &[("etag", "\"v2\"")], "updated"),
        ];
        let (client, requests) = mock(Duration::from_secs(2), redirects);
        seed(&client, "2024-01-01T00:00:00.000Z");
        let page = client.get(PRODUCT, Duration::ZERO).await.unwrap();
        assert_eq!(
            page.url.as_str(),
            "https://www.canyon.com/en-de/new-product"
        );
        assert_eq!(page.body, "updated");
        assert_eq!(
            client.cache.get(PRODUCT).unwrap().unwrap().etag.as_deref(),
            Some("\"v2\"")
        );
        {
            let requests = requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[0].1[header::IF_NONE_MATCH], "\"v1\"");
            assert!(!requests[1].1.contains_key(header::IF_NONE_MATCH));
            assert!(!requests[1].1.contains_key(header::IF_MODIFIED_SINCE));
            assert!(requests[1].2 - requests[0].2 >= Duration::from_secs(2));
        }
        for (location, code) in [
            ("/on/demandware.store/private", "ROBOTS_DISALLOWED"),
            ("https://example.com/", "URL_NOT_ALLOWED"),
            ("", "INVALID_REDIRECT"),
        ] {
            let (client, requests) = mock(
                Duration::ZERO,
                vec![response(302, &[("location", location)], "")],
            );
            seed(&client, "2024-01-01T00:00:00.000Z");
            assert_eq!(
                client.get(PRODUCT, Duration::ZERO).await.unwrap_err().code,
                code
            );
            assert_eq!(requests.lock().unwrap().len(), 1);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn spaces_concurrent_requests() {
        let interval = Duration::from_millis(80);
        let (client, requests) = mock(
            interval,
            vec![response(200, &[], "a"), response(200, &[], "b")],
        );
        let (a, b) = tokio::join!(
            client.get(PRODUCT, Duration::ZERO),
            client.get("https://www.canyon.com/en-de/other", Duration::ZERO),
        );
        assert_eq!(a.unwrap().body, "a");
        assert_eq!(b.unwrap().body, "b");
        let starts = requests.lock().unwrap();
        assert!(starts[1].2 - starts[0].2 >= interval);
    }

    #[tokio::test(start_paused = true)]
    async fn honors_courtesy_delay_and_retry_after() {
        let interval = Duration::from_millis(1200);
        let (client, requests) = mock(
            interval,
            vec![
                response(429, &[], ""),
                response(503, &[("retry-after", "4")], ""),
                response(200, &[], "ok"),
            ],
        );
        assert_eq!(
            client.get(PRODUCT, Duration::ZERO).await.unwrap().body,
            "ok"
        );
        let starts = requests.lock().unwrap();
        assert!(starts[1].2 - starts[0].2 >= interval);
        assert!(starts[2].2 - starts[1].2 >= Duration::from_secs(4));
    }

    #[tokio::test(start_paused = true)]
    async fn serves_stale_only_for_temporary_failure() {
        let (client, requests) = mock(
            Duration::ZERO,
            (0..3).map(|_| response(503, &[], "")).collect(),
        );
        seed(&client, "2024-01-01T00:00:00.000Z");
        let stale = client.get(PRODUCT, Duration::ZERO).await.unwrap();
        assert_eq!(stale.body, "cached");
        assert_eq!(stale.fetched_at, "2024-01-01T00:00:00.000Z");
        assert!(stale.from_cache && stale.stale);
        assert_eq!(requests.lock().unwrap().len(), 3);

        let (client, requests) = mock(Duration::ZERO, vec![response(404, &[], "")]);
        seed(&client, "2024-01-01T00:00:00.000Z");
        assert_eq!(
            client.get(PRODUCT, Duration::ZERO).await.unwrap_err().code,
            "HTTP_ERROR"
        );
        assert_eq!(requests.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_304_cannot_retimestamp_a_concurrently_replaced_body() {
        let path = tempfile::tempdir().unwrap();
        let writer = HttpCache::open(path.path().join("cache.sqlite")).unwrap();
        let reader = HttpCache::open(path.path().join("cache.sqlite")).unwrap();
        let mut entry = CacheEntry {
            body: "old".into(),
            etag: Some("\"old\"".into()),
            last_modified: None,
            fetched_at: "2024-01-01T00:00:00.000Z".into(),
        };
        writer.put(PRODUCT, &entry).unwrap();
        let cached = reader.get(PRODUCT).unwrap().unwrap();
        entry.body = "new".into();
        entry.etag = Some("\"new\"".into());
        writer.put(PRODUCT, &entry).unwrap();
        assert!(!reader.touch(PRODUCT, &cached, &timestamp()).unwrap());
        assert_eq!(reader.get(PRODUCT).unwrap().unwrap().body, "new");
    }
}
