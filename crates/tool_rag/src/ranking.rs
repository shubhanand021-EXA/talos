//! Baseline ranking: keyword match, capability match, receipt history.
//!
//! Filters first (disabled tools, capability allow-list, risk ceiling,
//! evidence requirement), then scores deterministically:
//!
//! - keyword score from the [`ScoreProvider`](crate::provider::ScoreProvider)
//!   (default: weighted token overlap),
//! - `+0.5` capability bonus when a capability filter matched,
//! - `+0.1` per successful receipt (capped at `+0.5`),
//! - `+0.15` evidence-backed manifest boost,
//! - `-0.15` per failed/denied receipt (capped at `-0.6`).
//!
//! Ties break by tool id ascending, so ranking is identical for fixed inputs.
//! Scores clamp at zero; every candidate explains itself in `reason`.

use std::collections::HashMap;

use talos_common::{Result, TalosError};

use crate::model::{
    required_approvals_for, risk_rank, IndexedTool, ReceiptStats, ToolCandidate, ToolQuery,
};
use crate::provider::{DocTerms, ScoreProvider};
use crate::tokenizer::tokenize;

/// Success boost per receipt (capped).
pub const SUCCESS_BOOST_PER_RECEIPT: f64 = 0.1;
/// Maximum success boost.
pub const SUCCESS_BOOST_CAP: f64 = 0.5;
/// Evidence-backed manifest boost.
pub const EVIDENCE_BOOST: f64 = 0.15;
/// Failure/denial penalty per receipt (capped).
pub const FAILURE_PENALTY_PER_RECEIPT: f64 = 0.15;
/// Maximum failure/denial penalty.
pub const FAILURE_PENALTY_CAP: f64 = 0.6;
/// Capability-filter match bonus.
pub const CAPABILITY_BONUS: f64 = 0.5;

/// Validate and normalize a raw query (intent 1..=1024 chars, top_k 1..=20).
pub fn validate_query(
    intent: &str,
    capabilities: Vec<String>,
    risk_ceiling: Option<String>,
    require_evidence: bool,
    top_k: Option<usize>,
) -> Result<ToolQuery> {
    let intent = intent.trim();
    if intent.is_empty() || intent.len() > 1024 {
        return Err(TalosError::Storage(
            "intent must be 1..=1024 chars".to_owned(),
        ));
    }
    let capabilities: Vec<String> = capabilities
        .into_iter()
        .map(|capability| capability.trim().to_owned())
        .filter(|capability| !capability.is_empty() && capability.len() <= 64)
        .collect();
    if let Some(ceiling) = &risk_ceiling {
        if risk_rank(ceiling) == u8::MAX {
            return Err(TalosError::Storage(
                "risk_ceiling must be low|medium|high|irreversible".to_owned(),
            ));
        }
    }
    Ok(ToolQuery {
        intent: intent.to_owned(),
        capabilities,
        risk_ceiling,
        require_evidence,
        top_k: top_k.unwrap_or(5).clamp(1, 20),
    })
}

/// Rank indexed tools for a query (deterministic for fixed inputs).
#[must_use]
pub fn rank(
    query: &ToolQuery,
    docs: &[IndexedTool],
    stats: &HashMap<String, ReceiptStats>,
    provider: &dyn ScoreProvider,
) -> Vec<ToolCandidate> {
    let query_tokens = tokenize(&query.intent);
    let ceiling = query.risk_ceiling.as_deref().map(risk_rank);
    let mut candidates = Vec::new();
    for doc in docs {
        if !doc.enabled {
            continue;
        }
        // Capability allow-list (case-insensitive).
        let matched_capability = if query.capabilities.is_empty() {
            None
        } else {
            let doc_caps: Vec<String> = doc
                .capabilities
                .iter()
                .map(|capability| capability.to_ascii_lowercase())
                .collect();
            query
                .capabilities
                .iter()
                .find(|wanted| {
                    doc_caps
                        .iter()
                        .any(|have| have == &wanted.to_ascii_lowercase())
                })
                .cloned()
        };
        if !query.capabilities.is_empty() && matched_capability.is_none() {
            continue;
        }
        // Risk ceiling (unknown manifest risk never passes).
        let doc_rank = risk_rank(&doc.risk_level);
        if doc_rank == u8::MAX {
            continue;
        }
        if let Some(ceiling) = ceiling {
            if doc_rank > ceiling {
                continue;
            }
        }
        let receipt = stats.get(&doc.tool_id).copied().unwrap_or_default();
        // Evidence requirement: manifest flag or a successful receipt.
        if query.require_evidence && !doc.requires_evidence && receipt.success == 0 {
            continue;
        }
        let terms = DocTerms {
            name: tokenize(&format!("{} {}", doc.tool_id, doc.name)),
            capabilities: tokenize(&doc.capabilities.join(" ")),
            description: tokenize(&doc.description),
            text: tokenize(&doc.text),
        };
        let keyword = provider.score(&query_tokens, &terms);
        let mut score = keyword;
        let mut reasons: Vec<String> = Vec::new();
        let mut matched_fields: Vec<String> = Vec::new();
        for (field, tokens) in [
            ("name", &terms.name),
            ("capability", &terms.capabilities),
            ("description", &terms.description),
            ("text", &terms.text),
        ] {
            if query_tokens.iter().any(|token| tokens.contains(token)) {
                matched_fields.push(field.to_owned());
            }
        }
        if keyword > 0.0 {
            reasons.push(format!(
                "keyword overlap {} token(s)",
                query_tokens
                    .iter()
                    .filter(|token| {
                        terms.name.contains(token)
                            || terms.capabilities.contains(token)
                            || terms.description.contains(token)
                            || terms.text.contains(token)
                    })
                    .count()
            ));
        }
        if let Some(capability) = matched_capability {
            score += CAPABILITY_BONUS;
            reasons.push(format!("capability match: {capability}"));
        }
        if receipt.success > 0 {
            let boost = (f64::from(receipt.success as u32) * SUCCESS_BOOST_PER_RECEIPT)
                .min(SUCCESS_BOOST_CAP);
            score += boost;
            reasons.push(format!("boost: {} successful receipt(s)", receipt.success));
        }
        if doc.requires_evidence {
            score += EVIDENCE_BOOST;
            reasons.push("boost: evidence-backed manifest".to_owned());
        }
        let bad = receipt.failed + receipt.denied;
        if bad > 0 {
            let penalty = (bad as f64 * FAILURE_PENALTY_PER_RECEIPT).min(FAILURE_PENALTY_CAP);
            score -= penalty;
            reasons.push(format!("penalty: {bad} failed/denied receipt(s)"));
        }
        if reasons.is_empty() {
            reasons.push("no lexical match; baseline candidate".to_owned());
        }
        reasons.push(format!(
            "risk {} within ceiling {}",
            doc.risk_level,
            query.risk_ceiling.as_deref().unwrap_or("any")
        ));
        candidates.push(ToolCandidate {
            tool_id: doc.tool_id.clone(),
            score: (score.max(0.0) * 100.0).round() / 100.0,
            reason: reasons.join("; "),
            matched_fields,
            risk_level: doc.risk_level.clone(),
            required_approvals: required_approvals_for(
                &doc.risk_level,
                doc.requires_evidence,
                doc.requires_compensation,
            ),
        });
    }
    candidates.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.tool_id.cmp(&right.tool_id))
    });
    candidates.truncate(query.top_k);
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::LocalLexicalProvider;
    use std::collections::HashMap;

    fn doc(id: &str, caps: &[&str], risk: &str, enabled: bool) -> IndexedTool {
        IndexedTool {
            tool_id: id.to_owned(),
            name: id.to_owned(),
            description: format!("Use {id} for work"),
            capabilities: caps
                .iter()
                .map(|capability| (*capability).to_owned())
                .collect(),
            risk_level: risk.to_owned(),
            reversible: true,
            requires_evidence: false,
            requires_compensation: false,
            enabled,
            text: format!("Use {id} for work"),
        }
    }

    fn query(intent: &str) -> ToolQuery {
        validate_query(intent, vec![], None, false, Some(5)).expect("valid")
    }

    #[test]
    fn invalid_queries_are_rejected() {
        assert!(validate_query("", vec![], None, false, None).is_err());
        assert!(validate_query("x", vec![], Some("bogus".to_owned()), false, None).is_err());
        let clamped = validate_query("x", vec![], None, false, Some(99)).expect("clamps");
        assert_eq!(clamped.top_k, 20);
    }

    #[test]
    fn capability_filter_selects_matching_tools() {
        let docs = vec![
            doc("fs.read", &["ReadFile"], "Low", true),
            doc("browser.click", &["OpenBrowser"], "Medium", true),
        ];
        let filtered = validate_query(
            "click something",
            vec!["OpenBrowser".to_owned()],
            None,
            false,
            Some(5),
        )
        .expect("valid");
        let ranked = rank(&filtered, &docs, &HashMap::new(), &LocalLexicalProvider);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].tool_id, "browser.click");
        assert!(ranked[0].reason.contains("capability match"));
    }

    #[test]
    fn risk_ceiling_filters_unsafe_candidates() {
        let docs = vec![
            doc("fs.read", &["ReadFile"], "Low", true),
            doc("process.exec", &["ExecuteCommand"], "High", true),
        ];
        let capped = validate_query("run", vec![], Some("medium".to_owned()), false, Some(5))
            .expect("valid");
        let ranked = rank(&capped, &docs, &HashMap::new(), &LocalLexicalProvider);
        assert!(ranked
            .iter()
            .all(|candidate| candidate.tool_id != "process.exec"));
        assert!(ranked
            .iter()
            .any(|candidate| candidate.tool_id == "fs.read"));
    }

    #[test]
    fn failed_receipts_reduce_ranking() {
        let docs = vec![
            doc("fs.read", &["ReadFile"], "Low", true),
            doc("fs.list", &["ReadFile"], "Low", true),
        ];
        let mut stats = HashMap::new();
        stats.insert(
            "fs.list".to_owned(),
            ReceiptStats {
                success: 0,
                failed: 4,
                denied: 0,
                disabled: 0,
            },
        );
        let ranked = rank(&query("read"), &docs, &stats, &LocalLexicalProvider);
        let read = ranked
            .iter()
            .find(|candidate| candidate.tool_id == "fs.read")
            .expect("read");
        let list = ranked
            .iter()
            .find(|candidate| candidate.tool_id == "fs.list")
            .expect("list");
        assert!(read.score > list.score);
        assert!(list.reason.contains("penalty"));
    }

    #[test]
    fn successful_receipts_and_evidence_boost() {
        let mut enabled_doc = doc("fs.write", &["WriteFile"], "Medium", true);
        enabled_doc.requires_evidence = true;
        let docs = vec![doc("fs.read", &["ReadFile"], "Low", true), enabled_doc];
        let mut stats = HashMap::new();
        stats.insert(
            "fs.write".to_owned(),
            ReceiptStats {
                success: 3,
                failed: 0,
                denied: 0,
                disabled: 0,
            },
        );
        let ranked = rank(&query("write file"), &docs, &stats, &LocalLexicalProvider);
        let write = ranked
            .iter()
            .find(|candidate| candidate.tool_id == "fs.write")
            .expect("write");
        assert!(write.reason.contains("successful receipt"));
        assert!(write.reason.contains("evidence-backed"));
        assert!(write
            .required_approvals
            .contains(&"prior_evidence".to_owned()));
    }

    #[test]
    fn ranking_is_deterministic_for_fixed_inputs() {
        let docs = vec![
            doc("fs.read", &["ReadFile"], "Low", true),
            doc("fs.list", &["ReadFile"], "Low", true),
        ];
        let first = rank(
            &query("read files"),
            &docs,
            &HashMap::new(),
            &LocalLexicalProvider,
        );
        let second = rank(
            &query("read files"),
            &docs,
            &HashMap::new(),
            &LocalLexicalProvider,
        );
        assert_eq!(first, second);
    }

    #[test]
    fn disabled_tools_never_rank() {
        let docs = vec![doc("process.exec", &["ExecuteCommand"], "High", false)];
        let ranked = rank(
            &query("execute"),
            &docs,
            &HashMap::new(),
            &LocalLexicalProvider,
        );
        assert!(ranked.is_empty());
    }
}
