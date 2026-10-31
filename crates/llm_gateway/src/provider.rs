//! Provider trait + structured gateway errors.

use async_trait::async_trait;

use crate::model::{LlmRequest, LlmResponse, LlmStream};

/// Structured gateway failures. Secret values are never embedded.
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    /// Caller-supplied request failed validation.
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    /// No provider serves the requested model.
    #[error("model unavailable: {0}")]
    ModelUnavailable(String),
    /// Provider rejected credentials. The key value is never included.
    #[error("provider authentication failed")]
    AuthFailure,
    /// Provider rate limit hit.
    #[error("provider rate limited")]
    RateLimited,
    /// Request exceeded its timeout.
    #[error("provider request timed out")]
    Timeout,
    /// Task budget exhausted before the call.
    #[error("task model budget exceeded")]
    BudgetExceeded,
    /// Policy denied the call (reason only, no content).
    #[error("policy denied model call: {0}")]
    PolicyDenied(String),
    /// Provider secret missing from the OS keychain.
    #[error("provider secret unavailable")]
    SecretUnavailable,
    /// Transient provider/transport failure (safe to retry).
    #[error("transient provider failure: {0}")]
    Transient(String),
    /// Non-retryable provider failure.
    #[error("provider failure: {0}")]
    Provider(String),
    /// Storage failure while recording usage/evidence.
    #[error("storage failure: {0}")]
    Storage(String),
}

impl LlmError {
    /// True when retrying with backoff may succeed.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            LlmError::Transient(_) | LlmError::Timeout | LlmError::RateLimited
        )
    }
}

impl From<LlmError> for talos_common::TalosError {
    fn from(err: LlmError) -> Self {
        talos_common::TalosError::Internal(err.to_string())
    }
}

/// Adapter implemented by every model provider.
#[async_trait]
pub trait LlmProvider: Send + Sync + std::fmt::Debug {
    /// Canonical provider id (`openai`, `anthropic`, `ollama`, `custom`).
    fn provider_id(&self) -> &str;
    /// True when this adapter can serve `model`.
    fn supports_model(&self, model: &str) -> bool;
    /// True when [`LlmProvider::stream`] yields deltas.
    fn supports_streaming(&self) -> bool;
    /// True when the adapter translates tool schemas + tool-call responses.
    fn supports_tool_calls(&self) -> bool;
    /// True when the adapter accepts image inputs.
    fn supports_vision(&self) -> bool;
    /// Best-effort catalog refresh (Ollama `/api/tags`); default is a no-op.
    async fn refresh_models(&self) -> Result<Vec<String>, LlmError> {
        Ok(Vec::new())
    }
    /// Models discovered by the last refresh (default: none).
    fn discovered_models(&self) -> Vec<String> {
        Vec::new()
    }
    /// One-shot completion.
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse, LlmError>;
    /// Streaming completion.
    async fn stream(&self, request: LlmRequest) -> Result<LlmStream, LlmError>;
}
