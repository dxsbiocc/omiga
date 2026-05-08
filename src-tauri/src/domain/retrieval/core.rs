use super::types::{RetrievalError, RetrievalProviderOutput, RetrievalRequest};
use crate::domain::tools::ToolContext;
use async_trait::async_trait;

#[async_trait]
pub trait RetrievalProvider: Send + Sync {
    async fn execute(
        &self,
        ctx: &ToolContext,
        request: RetrievalRequest,
    ) -> Result<RetrievalProviderOutput, RetrievalError>;
}

#[derive(Debug, Clone)]
pub struct RetrievalCore<P> {
    provider: P,
}

impl<P> RetrievalCore<P> {
    pub fn new(provider: P) -> Self {
        Self { provider }
    }

    pub fn provider(&self) -> &P {
        &self.provider
    }
}

impl<P> RetrievalCore<P>
where
    P: RetrievalProvider,
{
    pub async fn execute(
        &self,
        ctx: &ToolContext,
        request: RetrievalRequest,
    ) -> Result<RetrievalProviderOutput, RetrievalError> {
        self.provider.execute(ctx, request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::retrieval::types::{
        RetrievalOperation, RetrievalProviderKind, RetrievalResponse, RetrievalTool,
    };

    #[derive(Debug, Clone)]
    struct EchoProvider;

    #[async_trait]
    impl RetrievalProvider for EchoProvider {
        async fn execute(
            &self,
            _ctx: &ToolContext,
            request: RetrievalRequest,
        ) -> Result<RetrievalProviderOutput, RetrievalError> {
            Ok(RetrievalResponse {
                operation: request.operation,
                category: request.category,
                source: request.source.clone(),
                effective_source: request.source,
                provider: RetrievalProviderKind::Builtin,
                plugin: None,
                items: Vec::new(),
                detail: None,
                total: None,
                notes: vec!["echo".to_string()],
                raw: None,
            }
            .into())
        }
    }

    #[tokio::test]
    async fn core_delegates_to_provider() {
        let core = RetrievalCore::new(EchoProvider);
        let ctx = ToolContext::new("/tmp");
        let response = core
            .execute(
                &ctx,
                RetrievalRequest {
                    request_id: "r1".to_string(),
                    tool: RetrievalTool::Search,
                    operation: RetrievalOperation::Search,
                    category: "web".to_string(),
                    source: "ddg".to_string(),
                    subcategory: None,
                    query: Some("rust".to_string()),
                    id: None,
                    url: None,
                    result: None,
                    params: None,
                    max_results: Some(5),
                    prompt: None,
                    web: None,
                },
            )
            .await
            .unwrap();

        let RetrievalProviderOutput::Response(response) = response else {
            panic!("expected response output");
        };
        assert_eq!(response.source, "ddg");
        assert_eq!(response.notes, vec!["echo".to_string()]);
    }
}
