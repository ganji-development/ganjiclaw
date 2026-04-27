//! REST API handlers for skill management on the web dashboard.
//!
//! Routes (all bearer-token authenticated except where noted):
//! - `GET    /api/skills`                    — list installed skills
//! - `POST   /api/skills/install`            — install from clawhub/git/local
//! - `DELETE /api/skills/{id}`               — uninstall (rm -rf the skill dir)
//! - `POST   /api/skills/{id}/enable`        — drop from `disabled_skills`
//! - `POST   /api/skills/{id}/disable`       — append to `disabled_skills`
//! - `GET    /api/skills/registry/search`    — proxy to clawhub.ai/api/v1/search
//!
//! Skills hot-reload for free: every agent turn calls
//! `load_skills_with_config`, which re-scans the workspace skills dir and
//! filters on the live `disabled_skills` list. No registry refresh is
//! needed after install/uninstall/enable/disable.

use crate::AppState;
use crate::api::require_auth;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::PathBuf;
use zeroclaw_runtime::skills;

#[derive(Deserialize)]
pub struct InstallBody {
    /// `clawhub:<slug>`, an `https://clawhub.ai/...` URL, a git URL
    /// (`git@host:repo.git`, `*.git`, `git://...`, generic http(s)),
    /// or a local filesystem path.
    pub source: String,
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String,
}

fn workspace_skills_dir(state: &AppState) -> PathBuf {
    state.config.lock().workspace_dir.join("skills")
}

fn allow_scripts(state: &AppState) -> bool {
    state.config.lock().skills.allow_scripts
}

fn skill_to_json(skill: &skills::Skill, disabled: &HashSet<String>) -> serde_json::Value {
    let id = skills::skill_dir_name(skill);
    let enabled = id
        .as_ref()
        .map(|n| !disabled.contains(n))
        .unwrap_or(true);
    let location = skill
        .location
        .as_ref()
        .map(|p| p.display().to_string());
    serde_json::json!({
        "id": id,
        "name": skill.name,
        "description": skill.description,
        "version": skill.version,
        "author": skill.author,
        "tags": skill.tags,
        "tool_count": skill.tools.len(),
        "prompt_count": skill.prompts.len(),
        "location": location,
        "enabled": enabled,
    })
}

/// GET /api/skills — list installed skills (workspace + open-skills).
pub async fn handle_api_skills_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    // For the dashboard list we want to show ALL skills including disabled
    // ones (so the user can re-enable them). load_skills_with_config filters
    // disabled, so we re-load with a config snapshot that has an empty list.
    let mut config_snapshot = state.config.lock().clone();
    let disabled: HashSet<String> = config_snapshot
        .skills
        .disabled_skills
        .iter()
        .cloned()
        .collect();
    config_snapshot.skills.disabled_skills.clear();

    let skills_loaded =
        skills::load_skills_with_config(&config_snapshot.workspace_dir, &config_snapshot);

    let payload: Vec<serde_json::Value> = skills_loaded
        .iter()
        .map(|s| skill_to_json(s, &disabled))
        .collect();

    Json(serde_json::json!({ "skills": payload })).into_response()
}

/// POST /api/skills/install — install a skill from clawhub / git / local.
pub async fn handle_api_skills_install(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<InstallBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let source = body.source.trim().to_string();
    if source.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "source is required"})),
        )
            .into_response();
    }

    let skills_dir = workspace_skills_dir(&state);
    let allow_scripts = allow_scripts(&state);

    // Filesystem + network work: run on the blocking pool so we don't stall
    // the async runtime. install_clawhub_skill_source uses reqwest::blocking,
    // and install_git_skill_source spawns `git`.
    let result = tokio::task::spawn_blocking(move || {
        skills::install_skill_dispatch(&source, &skills_dir, allow_scripts)
    })
    .await;

    match result {
        Ok(Ok((path, files_scanned))) => Json(serde_json::json!({
            "status": "ok",
            "installed_path": path.display().to_string(),
            "files_scanned": files_scanned,
        }))
        .into_response(),
        Ok(Err(e)) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("install failed: {e}")})),
        )
            .into_response(),
        Err(join_err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("install task failed: {join_err}")})),
        )
            .into_response(),
    }
}

/// DELETE /api/skills/{id} — uninstall by directory id.
pub async fn handle_api_skills_uninstall(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let skills_dir = workspace_skills_dir(&state);
    let id_for_task = id.clone();
    let result =
        tokio::task::spawn_blocking(move || skills::uninstall_skill_by_dir_name(&skills_dir, &id_for_task))
            .await;

    match result {
        Ok(Ok(())) => {
            // Also strip from disabled_skills so a future re-install isn't
            // dead-on-arrival. Persist the config so the change survives.
            let mut cfg = state.config.lock().clone();
            let before = cfg.skills.disabled_skills.len();
            cfg.skills.disabled_skills.retain(|n| n != &id);
            if cfg.skills.disabled_skills.len() != before {
                if let Err(e) = cfg.save().await {
                    tracing::warn!("uninstall succeeded but failed to persist config: {e}");
                }
                *state.config.lock() = cfg;
            }
            Json(serde_json::json!({"status": "ok"})).into_response()
        }
        Ok(Err(e)) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("uninstall failed: {e}")})),
        )
            .into_response(),
        Err(join_err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("uninstall task failed: {join_err}")})),
        )
            .into_response(),
    }
}

async fn set_skill_disabled(state: AppState, id: String, disabled: bool) -> axum::response::Response {
    if id.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "skill id is required"})),
        )
            .into_response();
    }

    let mut cfg = state.config.lock().clone();
    let was_disabled = cfg.skills.disabled_skills.iter().any(|n| n == &id);

    if disabled && !was_disabled {
        cfg.skills.disabled_skills.push(id);
    } else if !disabled && was_disabled {
        cfg.skills.disabled_skills.retain(|n| n != &id);
    } else {
        // No change — return current state without rewriting config.
        return Json(serde_json::json!({"status": "ok", "changed": false })).into_response();
    }

    if let Err(e) = cfg.save().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("failed to persist config: {e}")})),
        )
            .into_response();
    }
    *state.config.lock() = cfg;

    Json(serde_json::json!({"status": "ok", "changed": true })).into_response()
}

/// POST /api/skills/{id}/enable
pub async fn handle_api_skills_enable(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    set_skill_disabled(state, id, false).await
}

/// POST /api/skills/{id}/disable
pub async fn handle_api_skills_disable(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    set_skill_disabled(state, id, true).await
}

/// GET /api/skills/registry/search?q=... — proxy to clawhub.ai search.
///
/// We proxy from the daemon (rather than letting the browser call clawhub
/// directly) for two reasons: (1) clawhub does not advertise a CORS policy
/// that permits the dashboard origin, (2) keeps "all clawhub interactions
/// happen in the daemon" as a single rule.
pub async fn handle_api_skills_search(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<SearchQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    match skills::search_clawhub_registry(&query.q).await {
        Ok(value) => Json(value).into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": format!("clawhub search failed: {e}")})),
        )
            .into_response(),
    }
}
