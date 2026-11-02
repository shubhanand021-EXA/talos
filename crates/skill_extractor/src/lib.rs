//! GEPA skill learning: extract reusable procedures from verified tasks,
//! evaluate recurring patterns every N tool calls, and retrieve published
//! skills for context assembly.
//!
//! The loop is closed and deterministic: extraction is a pure heuristic over
//! the `tool_receipt` evidence chain (no LLM — same chain, same document),
//! evaluation runs on ledger watermarks, and retrieval filters to published
//! skills whose permission ceiling covers the task risk.

pub mod extract;
pub mod gepa;
pub mod retrieve;

pub use extract::{
    build_skill_document, ceiling_for, extract_skill_for_task, ordered_calls_for_task,
    skill_markdown, slug, triggers_for, ExtractionOutcome, OrderedCall,
};
pub use gepa::{
    evaluate, evaluation_due, maybe_evaluate, watermark, EvaluationReport, GepaConfig, SkillUse,
};
pub use retrieve::{ceiling_covers, gather_published_skills, published_skill_tools, SkillSegment};
