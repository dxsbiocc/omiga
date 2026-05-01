//! Public literature-source search/fetch dispatch.

use super::{
    LiteraturePaper, LiteratureSearchArgs, LiteratureSearchResponse, PublicLiteratureClient,
    PublicLiteratureSource,
};

impl PublicLiteratureClient {
    pub async fn search(
        &self,
        source: PublicLiteratureSource,
        args: LiteratureSearchArgs,
    ) -> Result<LiteratureSearchResponse, String> {
        if args.query.trim().len() < 2 {
            return Err("literature search query must contain at least 2 characters".to_string());
        }
        match source {
            PublicLiteratureSource::Arxiv => self.search_arxiv(args).await,
            PublicLiteratureSource::Crossref => self.search_crossref(args).await,
            PublicLiteratureSource::OpenAlex => self.search_openalex(args).await,
            PublicLiteratureSource::Biorxiv => self.search_preprint(source, args).await,
            PublicLiteratureSource::Medrxiv => self.search_preprint(source, args).await,
        }
    }

    pub async fn fetch(
        &self,
        source: PublicLiteratureSource,
        identifier: &str,
    ) -> Result<LiteraturePaper, String> {
        let identifier = identifier.trim();
        if identifier.is_empty() {
            return Err(format!("{} fetch requires a non-empty id", source.as_str()));
        }
        match source {
            PublicLiteratureSource::Arxiv => self.fetch_arxiv(identifier).await,
            PublicLiteratureSource::Crossref => self.fetch_crossref(identifier).await,
            PublicLiteratureSource::OpenAlex => self.fetch_openalex(identifier).await,
            PublicLiteratureSource::Biorxiv | PublicLiteratureSource::Medrxiv => {
                self.fetch_preprint(source, identifier).await
            }
        }
    }
}
