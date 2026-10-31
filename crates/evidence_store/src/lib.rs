//! Foundational SQLite evidence store (append-only).
//!
//! Ledger verification arrives later; this crate only guarantees
//! durable, hash-linked inserts plus recency listings.

mod db;
mod models;
mod repo;

pub use db::EvidenceDb;
pub use models::{ClaimRow, EventRow, EvidenceRow, NewClaim, NewEvent, NewEvidence};
pub use repo::{
    insert_claim, insert_event, insert_evidence, list_recent_events, list_recent_evidence,
};
