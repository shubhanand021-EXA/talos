//! Model catalog + cheapest-adequate routing.
//!
//! The catalog is a hardcoded, reviewable baseline; operators extend it by
//! registering descriptors. Routing never issues a call without valid
//! task/agent linkage.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Static model capabilities + pricing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDescriptor {
    /// Provider id (`openai`, `anthropic`, `ollama`, `custom`).
    pub provider_id: String,
    /// Model id as the provider expects it.
    pub model_id: String,
    /// Maximum context window in tokens.
    pub context_window: u32,
    /// True when the model accepts tool schemas.
    pub supports_tools: bool,
    /// True when the model accepts image inputs.
    pub supports_vision: bool,
    /// True when the model streams deltas.
    pub supports_streaming: bool,
    /// USD per 1k input tokens.
    pub cost_per_1k_input: f64,
    /// USD per 1k output tokens.
    pub cost_per_1k_output: f64,
}

impl ModelDescriptor {
    /// Blended rate used for cheapest-first ordering.
    pub fn blended_rate(&self) -> f64 {
        self.cost_per_1k_input + self.cost_per_1k_output
    }
}

/// Task requirements driving model selection.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RoutingRequirement {
    /// Caller needs tool invocation.
    pub needs_tools: bool,
    /// Caller needs image understanding.
    pub needs_vision: bool,
    /// Caller wants deltas (falls back to unary when unmet).
    pub prefers_streaming: bool,
    /// Maximum acceptable blended per-1k rate (None = any).
    pub max_blended_rate: Option<f64>,
    /// Preferred provider (None = any).
    pub provider: Option<String>,
}

/// Cheapest-adequate model selection over a descriptor catalog.
#[derive(Debug, Default)]
pub struct ModelRouter {
    descriptors: HashMap<String, ModelDescriptor>,
}

impl ModelRouter {
    /// Empty router (callers register descriptors).
    pub fn new() -> Self {
        Self::default()
    }

    /// Router over the default catalog.
    pub fn with_defaults() -> Self {
        let mut router = Self::new();
        for descriptor in default_catalog() {
            router.register(descriptor);
        }
        router
    }

    /// Register (or replace) a descriptor keyed by `provider_id/model_id`.
    pub fn register(&mut self, descriptor: ModelDescriptor) {
        let key = format!("{}/{}", descriptor.provider_id, descriptor.model_id);
        self.descriptors.insert(key, descriptor);
    }

    /// Look up one descriptor.
    pub fn lookup(&self, provider_id: &str, model_id: &str) -> Option<&ModelDescriptor> {
        self.descriptors.get(&format!("{provider_id}/{model_id}"))
    }

    /// All descriptors, cheapest blended rate first.
    pub fn catalog(&self) -> Vec<&ModelDescriptor> {
        let mut all: Vec<&ModelDescriptor> = self.descriptors.values().collect();
        all.sort_by(|a, b| {
            a.blended_rate()
                .partial_cmp(&b.blended_rate())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        all
    }

    /// Select the cheapest descriptor satisfying `requirement`.
    /// Returns `None` when nothing qualifies.
    pub fn route(&self, requirement: &RoutingRequirement) -> Option<&ModelDescriptor> {
        self.catalog().into_iter().find(|d| {
            if requirement.needs_tools && !d.supports_tools {
                return false;
            }
            if requirement.needs_vision && !d.supports_vision {
                return false;
            }
            if let Some(cap) = requirement.max_blended_rate {
                if d.blended_rate() > cap {
                    return false;
                }
            }
            if let Some(provider) = &requirement.provider {
                if &d.provider_id != provider {
                    return false;
                }
            }
            true
        })
    }
}

/// Reviewable baseline catalog (prices are list rates at authoring time).
pub fn default_catalog() -> Vec<ModelDescriptor> {
    vec![
        ModelDescriptor {
            provider_id: "openai".to_owned(),
            model_id: "gpt-4o-mini".to_owned(),
            context_window: 128_000,
            supports_tools: true,
            supports_vision: true,
            supports_streaming: true,
            cost_per_1k_input: 0.00015,
            cost_per_1k_output: 0.0006,
        },
        ModelDescriptor {
            provider_id: "openai".to_owned(),
            model_id: "gpt-4o".to_owned(),
            context_window: 128_000,
            supports_tools: true,
            supports_vision: true,
            supports_streaming: true,
            cost_per_1k_input: 0.0025,
            cost_per_1k_output: 0.01,
        },
        ModelDescriptor {
            provider_id: "openai".to_owned(),
            model_id: "o3-mini".to_owned(),
            context_window: 200_000,
            supports_tools: true,
            supports_vision: false,
            supports_streaming: true,
            cost_per_1k_input: 0.0011,
            cost_per_1k_output: 0.0044,
        },
        ModelDescriptor {
            provider_id: "anthropic".to_owned(),
            model_id: "claude-3-5-haiku".to_owned(),
            context_window: 200_000,
            supports_tools: true,
            supports_vision: true,
            supports_streaming: true,
            cost_per_1k_input: 0.0008,
            cost_per_1k_output: 0.004,
        },
        ModelDescriptor {
            provider_id: "anthropic".to_owned(),
            model_id: "claude-sonnet-4-20250514".to_owned(),
            context_window: 200_000,
            supports_tools: true,
            supports_vision: true,
            supports_streaming: true,
            cost_per_1k_input: 0.003,
            cost_per_1k_output: 0.015,
        },
        ModelDescriptor {
            provider_id: "ollama".to_owned(),
            model_id: "llama3".to_owned(),
            context_window: 8_192,
            supports_tools: true,
            supports_vision: false,
            supports_streaming: true,
            cost_per_1k_input: 0.0,
            cost_per_1k_output: 0.0,
        },
    ]
}

/// Shared default catalog handle (rebuilt per call; cheap and reviewable).
pub fn default_registry() -> ModelRouter {
    ModelRouter::with_defaults()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_cheapest_adequate_model() {
        let router = ModelRouter::with_defaults();
        // Free local model wins the unconstrained route.
        let free = router.route(&RoutingRequirement::default()).unwrap();
        assert_eq!(free.blended_rate(), 0.0);
        // Vision need excludes o3-mini and llama3; cheapest vision model wins.
        let vision = router
            .route(&RoutingRequirement {
                needs_vision: true,
                ..Default::default()
            })
            .unwrap();
        assert!(vision.supports_vision);
        // Provider pinning is honored.
        let anthropic = router
            .route(&RoutingRequirement {
                provider: Some("anthropic".to_owned()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(anthropic.provider_id, "anthropic");
        // Impossible budgets route nowhere.
        let none = router.route(&RoutingRequirement {
            max_blended_rate: Some(-1.0),
            ..Default::default()
        });
        assert!(none.is_none());
    }

    #[test]
    fn tools_requirement_filters_correctly() {
        let mut router = ModelRouter::new();
        router.register(ModelDescriptor {
            provider_id: "x".to_owned(),
            model_id: "plain".to_owned(),
            context_window: 4096,
            supports_tools: false,
            supports_vision: false,
            supports_streaming: false,
            cost_per_1k_input: 0.0,
            cost_per_1k_output: 0.0,
        });
        assert!(router
            .route(&RoutingRequirement {
                needs_tools: true,
                ..Default::default()
            })
            .is_none());
        assert!(router.route(&RoutingRequirement::default()).is_some());
    }

    #[test]
    fn catalog_orders_cheapest_first() {
        let router = ModelRouter::with_defaults();
        let rates: Vec<f64> = router.catalog().iter().map(|d| d.blended_rate()).collect();
        let mut sorted = rates.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(rates, sorted);
    }
}
