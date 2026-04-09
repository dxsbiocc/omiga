//! Persisted toggles for post-turn LLM passes (SQLite `settings` table).

use crate::domain::persistence::SessionRepository;

pub const KEY_POST_TURN_SUMMARY: &str = "omiga.post_turn_summary_enabled";
pub const KEY_FOLLOW_UP_SUGGESTIONS: &str = "omiga.follow_up_suggestions_enabled";

fn parse_bool_setting(raw: Option<String>, default: bool) -> bool {
    match raw.as_deref().map(str::trim) {
        None | Some("") => default,
        Some(s) => match s.to_ascii_lowercase().as_str() {
            "false" | "0" | "no" | "off" => false,
            "true" | "1" | "yes" | "on" => true,
            _ => default,
        },
    }
}

/// `(post_turn_summary_enabled, follow_up_suggestions_enabled)` — default **true** when unset.
pub async fn load_post_turn_meta_flags(
    repo: &SessionRepository,
) -> Result<(bool, bool), sqlx::Error> {
    let s_summary = repo.get_setting(KEY_POST_TURN_SUMMARY).await?;
    let s_follow = repo.get_setting(KEY_FOLLOW_UP_SUGGESTIONS).await?;
    Ok((
        parse_bool_setting(s_summary, true),
        parse_bool_setting(s_follow, true),
    ))
}
