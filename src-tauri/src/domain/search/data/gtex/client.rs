//! GTEx Portal API HTTP client helpers.

use super::super::common;
use super::super::PublicDataClient;
#[cfg(debug_assertions)]
use super::mock;
use serde_json::Value as Json;

impl PublicDataClient {
    pub(in crate::domain::search::data::gtex) async fn gtex_get_json(
        &self,
        endpoint: &str,
        params: &[(String, String)],
    ) -> Result<Json, String> {
        #[cfg(debug_assertions)]
        if self.base_urls.gtex == "mock://gtex" {
            return mock::mock_gtex_json(endpoint, params).ok_or_else(|| {
                format!(
                    "debug GTEx mock has no fixture for endpoint `{endpoint}` with params {:?}",
                    params
                )
            });
        }

        let response = self
            .http
            .get(format!(
                "{}/{}",
                self.base_urls.gtex,
                endpoint.trim_start_matches('/')
            ))
            .query(params)
            .send()
            .await
            .map_err(|e| format!("GTEx Portal API {endpoint} request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("read GTEx Portal API {endpoint} response: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "GTEx Portal API {endpoint} returned HTTP {}: {}",
                status.as_u16(),
                common::truncate_for_error(&body)
            ));
        }
        serde_json::from_str(&body).map_err(|e| {
            format!(
                "parse GTEx Portal API {endpoint} JSON: {e}; body: {}",
                common::truncate_for_error(&body)
            )
        })
    }
}
