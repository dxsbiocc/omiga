//! Credential reference validation and minimal projection for retrieval plugins.

use super::types::RetrievalError;
use crate::domain::tools::WebSearchApiKeys;
use std::collections::HashMap;

pub const ALLOWED_CREDENTIAL_REFS: &[&str] = &[
    "tavily_api_key",
    "exa_api_key",
    "firecrawl_api_key",
    "firecrawl_url",
    "parallel_api_key",
    "semantic_scholar_api_key",
    "pubmed_api_key",
    "pubmed_email",
    "pubmed_tool_name",
];

pub fn is_allowed_credential_ref(value: &str) -> bool {
    let normalized = normalize_credential_ref(value);
    ALLOWED_CREDENTIAL_REFS
        .iter()
        .any(|item| *item == normalized)
}

pub fn normalize_credential_ref(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(['-', ' '], "_")
}

pub fn project_credentials(
    keys: &WebSearchApiKeys,
    category: &str,
    source: &str,
    required_refs: &[String],
    optional_refs: &[String],
) -> Result<HashMap<String, String>, RetrievalError> {
    let mut out = HashMap::new();
    let mut missing = Vec::new();

    for credential_ref in required_refs {
        let normalized = normalize_credential_ref(credential_ref);
        match credential_value(keys, &normalized) {
            Some(value) => {
                out.insert(normalized, value);
            }
            None => missing.push(normalized),
        }
    }

    if !missing.is_empty() {
        return Err(RetrievalError::MissingCredentials {
            category: category.to_string(),
            source_id: source.to_string(),
            refs: missing,
        });
    }

    for credential_ref in optional_refs {
        let normalized = normalize_credential_ref(credential_ref);
        if out.contains_key(&normalized) {
            continue;
        }
        if let Some(value) = credential_value(keys, &normalized) {
            out.insert(normalized, value);
        }
    }

    Ok(out)
}

fn credential_value(keys: &WebSearchApiKeys, credential_ref: &str) -> Option<String> {
    match credential_ref {
        "tavily_api_key" => clean(keys.tavily.as_deref()),
        "exa_api_key" => clean(keys.exa.as_deref()),
        "firecrawl_api_key" => clean(keys.firecrawl.as_deref()),
        "firecrawl_url" => clean(keys.firecrawl_url.as_deref()),
        "parallel_api_key" => clean(keys.parallel.as_deref()),
        "semantic_scholar_api_key" => clean(keys.semantic_scholar_api_key.as_deref()),
        "pubmed_api_key" => clean(keys.pubmed_api_key.as_deref()),
        "pubmed_email" => clean(keys.pubmed_email.as_deref()),
        "pubmed_tool_name" => clean(keys.pubmed_tool_name.as_deref()),
        _ => None,
    }
}

fn clean(value: Option<&str>) -> Option<String> {
    let trimmed = value?.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_refs_are_normalized_and_allowlisted() {
        assert!(is_allowed_credential_ref("PubMed API Key"));
        assert!(is_allowed_credential_ref("semantic-scholar-api-key"));
        assert!(!is_allowed_credential_ref("aws_secret_access_key"));
    }

    #[test]
    fn projects_only_requested_credentials() {
        let keys = WebSearchApiKeys {
            tavily: Some("tavily-secret".to_string()),
            pubmed_email: Some("user@example.org".to_string()),
            pubmed_api_key: Some("pubmed-secret".to_string()),
            ..WebSearchApiKeys::default()
        };

        let projected = project_credentials(
            &keys,
            "dataset",
            "mock_source",
            &["pubmed_email".to_string()],
            &["pubmed_api_key".to_string()],
        )
        .unwrap();

        assert_eq!(projected.len(), 2);
        assert_eq!(
            projected.get("pubmed_email").map(String::as_str),
            Some("user@example.org")
        );
        assert_eq!(
            projected.get("pubmed_api_key").map(String::as_str),
            Some("pubmed-secret")
        );
        assert!(!projected.contains_key("tavily_api_key"));
    }

    #[test]
    fn reports_missing_required_credentials() {
        let err = project_credentials(
            &WebSearchApiKeys::default(),
            "literature",
            "semantic_scholar",
            &["semantic_scholar_api_key".to_string()],
            &[],
        )
        .unwrap_err();

        match err {
            RetrievalError::MissingCredentials { refs, .. } => {
                assert_eq!(refs, vec!["semantic_scholar_api_key".to_string()]);
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
