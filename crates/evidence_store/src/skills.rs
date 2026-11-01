//! Skill registry: procedural knowledge distilled from successful tasks.
//!
//! Skills are project-level capabilities (`procedure_text`, required tools,
//! permission ceiling, test fixture). New skills are born `quarantined`;
//! promotion to `validated` / `published` is an explicit transition, never
//! implicit. The memory indexer embeds `procedure_text` for semantic recall.

use talos_common::{now_iso, Result, TalosError};

use crate::EvidenceDb;

/// Lifecycle status for a skill.
pub const SKILL_STATUSES: &[&str] = &["quarantined", "validated", "published"];

fn valid_id(what: &'static str, value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 128 {
        return Err(TalosError::InvalidId {
            what,
            reason: "must be 1..=128 chars".to_owned(),
        });
    }
    Ok(())
}

/// Full `skills` row.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct SkillRow {
    /// Skill identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// What the skill does.
    pub description: String,
    /// Step-by-step procedure text (embedded for semantic recall).
    pub procedure_text: String,
    /// Task the skill was distilled from, if any.
    pub source_task_id: Option<String>,
    /// Skill version.
    pub version: String,
    /// Test fixture reference proving the skill works.
    pub test_fixture_ref: Option<String>,
    /// JSON array of required tool ids.
    pub required_tools: String,
    /// Permission ceiling (`low` | `medium` | `high` | `irreversible`).
    pub permission_ceiling: String,
    /// Lifecycle status.
    pub status: String,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
}

/// Payload for [`create_skill`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSkill {
    /// Skill identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// What the skill does.
    pub description: String,
    /// Step-by-step procedure text.
    pub procedure_text: String,
    /// Task the skill was distilled from, if any.
    pub source_task_id: Option<String>,
    /// Skill version.
    pub version: String,
    /// Test fixture reference.
    pub test_fixture_ref: Option<String>,
    /// Required tool ids.
    pub required_tools: Vec<String>,
    /// Permission ceiling.
    pub permission_ceiling: String,
}

/// Create a skill in `quarantined` status (promotion is explicit).
pub async fn create_skill(db: &EvidenceDb, skill: NewSkill) -> Result<SkillRow> {
    valid_id("skill_id", &skill.id)?;
    if skill.name.trim().is_empty() || skill.name.len() > 256 {
        return Err(TalosError::InvalidId {
            what: "skill_name",
            reason: "must be 1..=256 chars".to_owned(),
        });
    }
    if skill.procedure_text.trim().is_empty() {
        return Err(TalosError::InvalidId {
            what: "procedure_text",
            reason: "must not be empty".to_owned(),
        });
    }
    if !["low", "medium", "high", "irreversible"].contains(&skill.permission_ceiling.as_str()) {
        return Err(TalosError::InvalidId {
            what: "permission_ceiling",
            reason: "must be low|medium|high|irreversible".to_owned(),
        });
    }
    if let Some(task_id) = skill.source_task_id.as_deref() {
        valid_id("source_task_id", task_id)?;
        let exists: Option<String> =
            sqlx::query_scalar("SELECT id FROM runtime_tasks WHERE id = ?")
                .bind(task_id)
                .fetch_optional(db.pool())
                .await
                .map_err(|err| TalosError::Storage(err.to_string()))?;
        if exists.is_none() {
            return Err(TalosError::InvalidId {
                what: "source_task_id",
                reason: "task does not exist".to_owned(),
            });
        }
    }
    let required_tools = serde_json::to_string(&skill.required_tools).map_err(TalosError::from)?;
    let created_at = now_iso();
    sqlx::query(
        "INSERT INTO skills
         (id, name, description, procedure_text, source_task_id, version,
          test_fixture_ref, required_tools, permission_ceiling, status, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'quarantined', ?)",
    )
    .bind(&skill.id)
    .bind(&skill.name)
    .bind(&skill.description)
    .bind(&skill.procedure_text)
    .bind(&skill.source_task_id)
    .bind(&skill.version)
    .bind(&skill.test_fixture_ref)
    .bind(&required_tools)
    .bind(&skill.permission_ceiling)
    .bind(&created_at)
    .execute(db.pool())
    .await
    .map_err(|err| TalosError::Storage(err.to_string()))?;
    Ok(SkillRow {
        id: skill.id,
        name: skill.name,
        description: skill.description,
        procedure_text: skill.procedure_text,
        source_task_id: skill.source_task_id,
        version: skill.version,
        test_fixture_ref: skill.test_fixture_ref,
        required_tools,
        permission_ceiling: skill.permission_ceiling,
        status: "quarantined".to_owned(),
        created_at,
    })
}

/// Fetch one skill by id.
pub async fn get_skill(db: &EvidenceDb, id: &str) -> Result<Option<SkillRow>> {
    valid_id("skill_id", id)?;
    sqlx::query_as::<_, SkillRow>("SELECT * FROM skills WHERE id = ?")
        .bind(id)
        .fetch_optional(db.pool())
        .await
        .map_err(|err| TalosError::Storage(err.to_string()))
}

/// List skills, newest last by rowid, bounded (optional status filter).
pub async fn list_skills(
    db: &EvidenceDb,
    status: Option<&str>,
    limit: i64,
) -> Result<Vec<SkillRow>> {
    let limit = limit.clamp(1, 500);
    if let Some(wanted) = status {
        if !SKILL_STATUSES.contains(&wanted) {
            return Err(TalosError::InvalidId {
                what: "status",
                reason: "must be quarantined|validated|published".to_owned(),
            });
        }
        sqlx::query_as::<_, SkillRow>(
            "SELECT * FROM skills WHERE status = ? ORDER BY rowid ASC LIMIT ?",
        )
        .bind(wanted)
        .bind(limit)
        .fetch_all(db.pool())
        .await
        .map_err(|err| TalosError::Storage(err.to_string()))
    } else {
        sqlx::query_as::<_, SkillRow>("SELECT * FROM skills ORDER BY rowid ASC LIMIT ?")
            .bind(limit)
            .fetch_all(db.pool())
            .await
            .map_err(|err| TalosError::Storage(err.to_string()))
    }
}

/// Explicit promotion: `quarantined` → `validated` → `published` (no skips,
/// no demotion through this path).
pub async fn update_skill_status(db: &EvidenceDb, id: &str, to: &str) -> Result<SkillRow> {
    valid_id("skill_id", id)?;
    let row = get_skill(db, id)
        .await?
        .ok_or_else(|| TalosError::InvalidId {
            what: "skill_id",
            reason: "skill does not exist".to_owned(),
        })?;
    let allowed = matches!(
        (row.status.as_str(), to),
        ("quarantined", "validated") | ("validated", "published")
    );
    if !allowed {
        return Err(TalosError::InvalidId {
            what: "status",
            reason: "only quarantined->validated->published is allowed".to_owned(),
        });
    }
    sqlx::query("UPDATE skills SET status = ? WHERE id = ?")
        .bind(to)
        .bind(id)
        .execute(db.pool())
        .await
        .map_err(|err| TalosError::Storage(err.to_string()))?;
    get_skill(db, id)
        .await?
        .ok_or_else(|| TalosError::Storage("skill missing after update".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn db() -> EvidenceDb {
        EvidenceDb::connect_memory().await.expect("memory db")
    }

    fn skill(id: &str) -> NewSkill {
        NewSkill {
            id: id.to_owned(),
            name: "read then hash".to_owned(),
            description: "Read a file and hash it.".to_owned(),
            procedure_text: "1. fs.read the path. 2. Record the sha256 hash.".to_owned(),
            source_task_id: None,
            version: "0.1.0".to_owned(),
            test_fixture_ref: None,
            required_tools: vec!["fs.read".to_owned()],
            permission_ceiling: "low".to_owned(),
        }
    }

    #[tokio::test]
    async fn creates_quarantined_by_default() {
        let db = db().await;
        let row = create_skill(&db, skill("skill-1")).await.expect("create");
        assert_eq!(row.status, "quarantined");
        let fetched = get_skill(&db, "skill-1").await.expect("get").expect("row");
        assert_eq!(fetched.procedure_text, row.procedure_text);
    }

    #[tokio::test]
    async fn promotion_is_explicit_and_ordered() {
        let db = db().await;
        create_skill(&db, skill("skill-2")).await.expect("create");
        // No skips, no demotion.
        assert!(update_skill_status(&db, "skill-2", "published")
            .await
            .is_err());
        let validated = update_skill_status(&db, "skill-2", "validated")
            .await
            .expect("promote");
        assert_eq!(validated.status, "validated");
        let published = update_skill_status(&db, "skill-2", "published")
            .await
            .expect("publish");
        assert_eq!(published.status, "published");
        assert!(update_skill_status(&db, "skill-2", "quarantined")
            .await
            .is_err());
        // Unknown status values are rejected.
        assert!(update_skill_status(&db, "skill-2", "archived")
            .await
            .is_err());
        assert!(list_skills(&db, Some("bogus"), 10).await.is_err());
    }

    #[tokio::test]
    async fn rejects_bad_input() {
        let db = db().await;
        assert!(create_skill(
            &db,
            NewSkill {
                id: String::new(),
                ..skill("x")
            }
        )
        .await
        .is_err());
        assert!(create_skill(
            &db,
            NewSkill {
                procedure_text: String::new(),
                ..skill("x")
            }
        )
        .await
        .is_err());
        assert!(create_skill(
            &db,
            NewSkill {
                permission_ceiling: "extreme".to_owned(),
                ..skill("x")
            }
        )
        .await
        .is_err());
        assert!(create_skill(
            &db,
            NewSkill {
                source_task_id: Some("missing".to_owned()),
                ..skill("x")
            }
        )
        .await
        .is_err());
        assert!(get_skill(&db, "").await.is_err());
    }
}
