//! Public data-source search/fetch dispatch.

use super::common::{DataRecord, DataSearchArgs, DataSearchResponse, PublicDataSource};
use super::PublicDataClient;

impl PublicDataClient {
    pub async fn search(
        &self,
        source: PublicDataSource,
        args: DataSearchArgs,
    ) -> Result<DataSearchResponse, String> {
        if args.query.trim().len() < 2 {
            return Err("data search query must contain at least 2 characters".to_string());
        }
        match source {
            PublicDataSource::Geo => self.search_geo(args).await,
            PublicDataSource::CbioPortal => self.search_cbioportal(args).await,
            PublicDataSource::Gtex => self.search_gtex(args).await,
            PublicDataSource::NcbiDatasets => self.search_ncbi_datasets(args).await,
            source => self.search_ena(source, args).await,
        }
    }

    pub async fn search_auto(&self, args: DataSearchArgs) -> Result<DataSearchResponse, String> {
        if args.query.trim().len() < 2 {
            return Err("data search query must contain at least 2 characters".to_string());
        }
        let max_results = args.normalized_max_results() as usize;
        let geo_args = args.clone();
        let ena_args = args.clone();
        let gtex_args = args.clone();
        let ncbi_datasets_args = args.clone();
        let (geo, ena, gtex, ncbi_datasets) = tokio::join!(
            self.search_geo(geo_args),
            self.search_ena(PublicDataSource::EnaStudy, ena_args),
            self.search_gtex(gtex_args),
            self.search_ncbi_datasets(ncbi_datasets_args)
        );

        let mut results = Vec::new();
        let mut total = 0u64;
        let mut saw_total = false;
        let mut notes = vec!["Combined GEO + ENA + GTEx + NCBI Datasets data search".to_string()];

        for response in [geo, ena, gtex, ncbi_datasets] {
            match response {
                Ok(response) => {
                    if let Some(count) = response.total {
                        total = total.saturating_add(count);
                        saw_total = true;
                    }
                    notes.extend(response.notes);
                    results.extend(response.results);
                }
                Err(err) => notes.push(format!("source failed: {err}")),
            }
        }

        results.truncate(max_results);
        Ok(DataSearchResponse {
            query: args.query.trim().to_string(),
            source: "auto".to_string(),
            total: saw_total.then_some(total),
            results,
            notes,
        })
    }

    pub async fn fetch(
        &self,
        source: PublicDataSource,
        identifier: &str,
    ) -> Result<DataRecord, String> {
        let identifier = identifier.trim();
        if identifier.is_empty() {
            return Err(format!("{} fetch requires a non-empty id", source.as_str()));
        }
        match source {
            PublicDataSource::Geo => self.fetch_geo(identifier).await,
            PublicDataSource::CbioPortal => self.fetch_cbioportal(identifier).await,
            PublicDataSource::Gtex => self.fetch_gtex(identifier).await,
            PublicDataSource::NcbiDatasets => self.fetch_ncbi_datasets(identifier).await,
            source => self.fetch_ena(source, identifier).await,
        }
    }
}
