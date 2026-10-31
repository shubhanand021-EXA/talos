//! Injectable HTTP layer. Providers talk to [`HttpClient`], never to the
//! network directly, so unit tests run hermetically against [`MockHttpClient`].

use async_trait::async_trait;

/// Minimal HTTP surface the providers need.
#[async_trait]
pub trait HttpClient: Send + Sync + std::fmt::Debug {
    /// POST a JSON body; returns (status, body bytes).
    async fn post_json(
        &self,
        url: &str,
        headers: Vec<(String, String)>,
        body: serde_json::Value,
    ) -> Result<(u16, Vec<u8>), String>;
    /// GET a URL; returns (status, body bytes).
    async fn get(
        &self,
        url: &str,
        headers: Vec<(String, String)>,
    ) -> Result<(u16, Vec<u8>), String>;
}

/// Live client backed by reqwest with a 60s timeout.
#[derive(Debug, Clone)]
pub struct ReqwestHttpClient {
    inner: reqwest::Client,
}

impl ReqwestHttpClient {
    /// Build with the gateway-standard timeout.
    pub fn new() -> Result<Self, String> {
        let inner = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|err| err.to_string())?;
        Ok(Self { inner })
    }
}

impl Default for ReqwestHttpClient {
    fn default() -> Self {
        Self::new().expect("reqwest client builds with default settings")
    }
}

#[async_trait]
impl HttpClient for ReqwestHttpClient {
    async fn post_json(
        &self,
        url: &str,
        headers: Vec<(String, String)>,
        body: serde_json::Value,
    ) -> Result<(u16, Vec<u8>), String> {
        let mut request = self.inner.post(url).json(&body);
        for (name, value) in headers {
            request = request.header(name, value);
        }
        let response = request.send().await.map_err(|err| {
            if err.is_timeout() {
                "timeout".to_owned()
            } else {
                err.to_string()
            }
        })?;
        let status = response.status().as_u16();
        let bytes = response.bytes().await.map_err(|err| err.to_string())?;
        Ok((status, bytes.to_vec()))
    }

    async fn get(
        &self,
        url: &str,
        headers: Vec<(String, String)>,
    ) -> Result<(u16, Vec<u8>), String> {
        let mut request = self.inner.get(url);
        for (name, value) in headers {
            request = request.header(name, value);
        }
        let response = request.send().await.map_err(|err| {
            if err.is_timeout() {
                "timeout".to_owned()
            } else {
                err.to_string()
            }
        })?;
        let status = response.status().as_u16();
        let bytes = response.bytes().await.map_err(|err| err.to_string())?;
        Ok((status, bytes.to_vec()))
    }
}

/// Canned-response client for hermetic tests.
#[derive(Debug, Default)]
pub struct MockHttpClient {
    /// Queued (status, body) pairs, consumed FIFO per request.
    pub responses: std::sync::Mutex<Vec<(u16, serde_json::Value)>>,
    /// Observed (method, url) pairs for assertions.
    pub calls: std::sync::Mutex<Vec<(String, String)>>,
    /// Error returned instead of a response when set.
    pub error: std::sync::Mutex<Option<String>>,
}

impl MockHttpClient {
    /// Create with one queued response.
    pub fn with_response(status: u16, body: serde_json::Value) -> Self {
        Self {
            responses: std::sync::Mutex::new(vec![(status, body)]),
            calls: std::sync::Mutex::new(Vec::new()),
            error: std::sync::Mutex::new(None),
        }
    }

    /// Queue another response.
    pub fn push_response(&self, status: u16, body: serde_json::Value) {
        self.responses.lock().expect("mock").push((status, body));
    }

    /// Number of observed calls.
    pub fn call_count(&self) -> usize {
        self.calls.lock().expect("mock").len()
    }

    fn next(&self, method: &str, url: &str) -> Result<(u16, Vec<u8>), String> {
        self.calls
            .lock()
            .expect("mock")
            .push((method.to_owned(), url.to_owned()));
        if let Some(err) = self.error.lock().expect("mock").clone() {
            return Err(err);
        }
        let (status, body) = if self.responses.lock().expect("mock").is_empty() {
            (500, serde_json::json!({"error": "no mock response"}))
        } else {
            self.responses.lock().expect("mock").remove(0)
        };
        let bytes = serde_json::to_vec(&body).expect("mock body serializes");
        Ok((status, bytes))
    }
}

#[async_trait]
impl HttpClient for MockHttpClient {
    async fn post_json(
        &self,
        url: &str,
        _headers: Vec<(String, String)>,
        _body: serde_json::Value,
    ) -> Result<(u16, Vec<u8>), String> {
        self.next("POST", url)
    }

    async fn get(
        &self,
        url: &str,
        _headers: Vec<(String, String)>,
    ) -> Result<(u16, Vec<u8>), String> {
        self.next("GET", url)
    }
}
