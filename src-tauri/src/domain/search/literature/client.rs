//! Public literature-source HTTP client construction.

use super::clean_optional;
use crate::domain::tools::ToolContext;
use std::time::Duration;

#[derive(Clone)]
pub struct PublicLiteratureClient {
    pub(super) http: reqwest::Client,
    pub(super) mailto: String,
}

impl PublicLiteratureClient {
    pub fn from_tool_context(ctx: &ToolContext) -> Result<Self, String> {
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(ctx.timeout_secs.clamp(5, 45)))
            .user_agent(format!(
                "Omiga-LiteratureSearch/{} (mailto:{})",
                env!("CARGO_PKG_VERSION"),
                clean_optional(ctx.web_search_api_keys.pubmed_email.as_deref())
                    .unwrap_or("omiga@example.invalid")
            ));
        if !ctx.web_use_proxy {
            builder = builder.no_proxy();
        }
        let http = builder
            .build()
            .map_err(|e| format!("build literature HTTP client: {e}"))?;
        Ok(Self {
            http,
            mailto: clean_optional(ctx.web_search_api_keys.pubmed_email.as_deref())
                .unwrap_or("omiga@example.invalid")
                .to_string(),
        })
    }

    #[cfg(test)]
    pub fn for_tests() -> Self {
        Self {
            http: reqwest::Client::new(),
            mailto: "omiga@example.invalid".to_string(),
        }
    }
}
