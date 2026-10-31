//! Typed identifier wrappers. IDs are non-empty strings (`<= 128` chars).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::TalosError;

macro_rules! define_id {
    ($name:ident, $what:literal, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Validate and wrap a raw identifier.
            pub fn new(raw: impl Into<String>) -> crate::error::Result<Self> {
                let value = raw.into();
                validate_id($what, &value)?;
                Ok(Self(value))
            }

            /// Borrow the inner string.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Consume the wrapper into a plain string.
            #[must_use]
            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = TalosError;

            fn from_str(raw: &str) -> Result<Self, Self::Err> {
                Self::new(raw.to_owned())
            }
        }
    };
}

fn validate_id(what: &'static str, value: &str) -> crate::error::Result<()> {
    if value.is_empty() {
        return Err(TalosError::InvalidId {
            what,
            reason: "identifier must not be empty".to_owned(),
        });
    }
    if value.len() > 128 {
        return Err(TalosError::InvalidId {
            what,
            reason: "identifier must be at most 128 chars".to_owned(),
        });
    }
    Ok(())
}

define_id!(AgentId, "agent_id", "Typed agent identifier.");
define_id!(TaskId, "task_id", "Typed task identifier.");
define_id!(EvidenceId, "evidence_id", "Typed evidence identifier.");
define_id!(
    CompensationPlanId,
    "compensation_plan_id",
    "Typed compensation-plan identifier."
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        let id = AgentId::new("agent-1").expect("valid id");
        assert_eq!(id.as_str(), "agent-1");
        assert_eq!(id.to_string(), "agent-1");
        let parsed: AgentId = "agent-1".parse().expect("parse works");
        assert_eq!(parsed, id);
    }

    #[test]
    fn ids_reject_empty_and_long() {
        assert!(AgentId::new("").is_err());
        assert!(TaskId::new("x".repeat(129)).is_err());
        assert!(EvidenceId::new("ok").is_ok());
    }

    #[test]
    fn ids_serialize_transparently() {
        let id = TaskId::new("task-1").expect("valid id");
        let json = serde_json::to_string(&id).expect("serializes");
        assert_eq!(json, "\"task-1\"");
    }
}
