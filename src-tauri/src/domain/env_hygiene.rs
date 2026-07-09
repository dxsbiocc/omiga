//! Environment variable filtering utilities for shell-export and subprocess launch hygiene.

use std::collections::BTreeMap;

pub type Pattern = String;

pub fn is_sensitive_env_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.contains("KEY") || upper.contains("SECRET") || upper.contains("TOKEN")
}

pub fn keep_exemptions_from(raw: Option<&str>) -> Vec<Pattern> {
    raw.unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter_map(|pattern| match pattern {
            "" => None,
            "*" => {
                tracing::warn!("忽略非法豁免模式 `*`：如需禁用过滤请显式配置，不支持全局通配");
                None
            }
            _ => Some(pattern.to_string()),
        })
        .collect()
}

pub fn filter_env_vars<I, K, V>(
    vars: I,
    keep_exemptions: &[Pattern],
) -> (Vec<(String, String)>, Vec<String>)
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    let mut kept = Vec::new();
    let mut dropped_names = Vec::new();

    for (name, value) in vars {
        let name = name.as_ref();
        let value = value.as_ref();
        if is_sensitive_env_name(name) && !is_exempt(name, keep_exemptions) {
            dropped_names.push(name.to_string());
            continue;
        }
        kept.push((name.to_string(), value.to_string()));
    }

    (kept, dropped_names)
}

fn is_exempt(name: &str, exemptions: &[Pattern]) -> bool {
    exemptions
        .iter()
        .any(|pattern| match pattern.strip_suffix('*') {
            Some(prefix) if !prefix.is_empty() => name.starts_with(prefix),
            Some(_) => false,
            None => pattern == name,
        })
}

pub fn shell_export_lines(env: &BTreeMap<String, String>) -> String {
    let keep_exemptions = keep_exemptions_from(std::env::var("OMIGA_ENV_KEEP").ok().as_deref());
    shell_export_lines_with_exemptions(env, &keep_exemptions)
}

pub(crate) fn shell_export_lines_with_exemptions(
    env: &BTreeMap<String, String>,
    keep_exemptions: &[Pattern],
) -> String {
    let safe_names = env
        .iter()
        .filter(|(name, _)| is_safe_shell_identifier(name))
        .map(|(name, value)| (name.as_str(), value.as_str()));
    let (kept, dropped_names) = filter_env_vars(safe_names, keep_exemptions);
    for name in dropped_names {
        tracing::debug!(name = %name, "filtered sensitive env var from shell export");
    }
    kept.into_iter()
        .map(|(key, value)| format!("export {key}={}", sh_quote(&value)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_safe_shell_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn map_from(values: &[(&str, &str)]) -> BTreeMap<String, String> {
        values
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn sensitive_env_name_matches_key_secret_token_case_insensitive() {
        assert!(is_sensitive_env_name("my_api_key"));
        assert!(is_sensitive_env_name("AWS_SECRET_ACCESS_KEY"));
        assert!(is_sensitive_env_name("gh_token"));
        assert!(!is_sensitive_env_name("PATH"));
        assert!(!is_sensitive_env_name("NORMAL"));
    }

    #[test]
    fn keep_exemptions_parses_comma_list_and_wildcard_patterns() {
        let raw = Some("  MY_API_KEY, HF_*,,  ,TOKEN_ONLY ");
        let exemptions = keep_exemptions_from(raw);
        assert_eq!(
            exemptions,
            vec![
                "MY_API_KEY".to_string(),
                "HF_*".to_string(),
                "TOKEN_ONLY".to_string()
            ]
        );
    }

    #[test]
    fn keep_exemptions_discards_empty_and_global_wildcard_patterns() {
        let exemptions = keep_exemptions_from(Some("*,   ,"));
        assert!(exemptions.is_empty());
    }

    #[test]
    fn filter_env_vars_uses_exemptions_and_returns_kept_and_dropped() {
        let env = map_from(&[
            ("MY_API_KEY", "a"),
            ("AWS_SECRET_ACCESS_KEY", "b"),
            ("GH_TOKEN", "c"),
            ("NORMAL_VAR", "d"),
        ]);

        let (kept, dropped) = filter_env_vars(
            env.iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
            &[],
        );
        assert_eq!(kept, vec![("NORMAL_VAR".to_string(), "d".to_string())]);
        assert_eq!(
            dropped,
            vec![
                "AWS_SECRET_ACCESS_KEY".to_string(),
                "GH_TOKEN".to_string(),
                "MY_API_KEY".to_string()
            ]
        );

        let exemptions = keep_exemptions_from(Some("MY_API_KEY,HF_*"));
        let (kept_with_exemptions, dropped_with_exemptions) = filter_env_vars(
            env.iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
            &exemptions,
        );
        assert_eq!(
            kept_with_exemptions,
            vec![
                ("MY_API_KEY".to_string(), "a".to_string()),
                ("NORMAL_VAR".to_string(), "d".to_string())
            ]
        );
        assert_eq!(
            dropped_with_exemptions,
            vec!["AWS_SECRET_ACCESS_KEY".to_string(), "GH_TOKEN".to_string()]
        );
    }

    #[test]
    fn filter_env_vars_still_filters_with_global_wildcard_exemption_and_honors_prefix_patterns() {
        let env = map_from(&[
            ("MY_API_KEY", "a"),
            ("AWS_SECRET_ACCESS_KEY", "b"),
            ("HF_TOKEN", "c"),
            ("NORMAL_VAR", "d"),
        ]);

        let (kept_star, dropped_star) = filter_env_vars(
            env.iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
            &keep_exemptions_from(Some("*")),
        );
        assert_eq!(kept_star, vec![("NORMAL_VAR".to_string(), "d".to_string())]);
        assert_eq!(
            dropped_star,
            vec![
                "AWS_SECRET_ACCESS_KEY".to_string(),
                "HF_TOKEN".to_string(),
                "MY_API_KEY".to_string()
            ]
        );

        let (kept_prefix, dropped_prefix) = filter_env_vars(
            env.iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
            &keep_exemptions_from(Some("HF_*")),
        );
        assert_eq!(
            kept_prefix,
            vec![
                ("HF_TOKEN".to_string(), "c".to_string()),
                ("NORMAL_VAR".to_string(), "d".to_string())
            ]
        );
        assert_eq!(
            dropped_prefix,
            vec![
                "AWS_SECRET_ACCESS_KEY".to_string(),
                "MY_API_KEY".to_string()
            ]
        );
    }

    #[test]
    fn shell_export_lines_filters_sensitive_env_and_honors_exemptions() {
        let env = map_from(&[
            ("MY_API_KEY", "a"),
            ("AWS_SECRET_ACCESS_KEY", "b"),
            ("GH_TOKEN", "c"),
            ("NORMAL_VAR", "d"),
        ]);
        let exports = shell_export_lines_with_exemptions(&env, &[]);
        assert_eq!(exports, "export NORMAL_VAR='d'");

        let exports = shell_export_lines_with_exemptions(&env, &["MY_API_KEY".to_string()]);
        let lines: Vec<_> = exports.lines().collect();
        assert_eq!(
            lines,
            vec!["export MY_API_KEY='a'", "export NORMAL_VAR='d'"]
        );
    }

    #[test]
    fn filter_env_vars_handles_empty_inputs() {
        let (kept, dropped) = filter_env_vars(std::iter::empty::<(String, String)>(), &[]);
        assert_eq!(kept, Vec::<(String, String)>::new());
        assert_eq!(dropped, Vec::<String>::new());
    }
}
