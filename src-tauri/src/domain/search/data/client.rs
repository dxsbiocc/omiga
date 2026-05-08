//! Public data-source HTTP client construction.

use super::common::{DataApiBaseUrls, EntrezSettings};
use crate::domain::tools::ToolContext;
use std::time::Duration;

#[derive(Clone)]
pub struct PublicDataClient {
    pub(in crate::domain::search::data) http: reqwest::Client,
    pub(in crate::domain::search::data) base_urls: DataApiBaseUrls,
    pub(in crate::domain::search::data) settings: EntrezSettings,
}

impl PublicDataClient {
    pub fn from_tool_context(ctx: &ToolContext) -> Result<Self, String> {
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(ctx.timeout_secs.clamp(5, 60)))
            .user_agent(format!("Omiga-DataSearch/{}", env!("CARGO_PKG_VERSION")));
        if !ctx.web_use_proxy {
            builder = builder.no_proxy();
        }
        let http = builder
            .build()
            .map_err(|e| format!("build data-source HTTP client: {e}"))?;
        Ok(Self {
            http,
            base_urls: ctx.data_api_base_urls.clone(),
            settings: EntrezSettings::from_keys(&ctx.web_search_api_keys),
        })
    }
}
