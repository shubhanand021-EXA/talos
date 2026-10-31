//! LLM provider gateway: BYOK model routing behind policy + evidence.
//!
//! [`LlmGateway`] is the single entry point: it validates the request,
//! enforces per-task budgets, gates through the [`policy_broker`], executes
//! via an [`LlmProvider`] adapter, and persists usage + interaction hashes
//! to the evidence store. Raw prompts and completions are never persisted.

pub mod budget;
pub mod gateway;
pub mod http;
pub mod model;
pub mod provider;
pub mod providers;
pub mod registry;

pub use budget::{BudgetTracker, MAX_SPEND_PRECISION_USD};
pub use gateway::{GatewayConfig, LlmGateway};
pub use http::{HttpClient, MockHttpClient, ReqwestHttpClient};
pub use model::{
    FinishReason, LlmMessage, LlmRequest, LlmRequestMetadata, LlmResponse, LlmRole, LlmStream,
    LlmStreamChunk, LlmToolCall, ProviderId, ProviderReceipt, TokenUsage, ToolSchema,
};
pub use provider::{LlmError, LlmProvider};
pub use registry::{default_registry, ModelDescriptor, ModelRouter, RoutingRequirement};
