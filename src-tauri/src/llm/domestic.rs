//! Domestic Chinese LLM providers
//!
//! Supported providers:
//! - Baidu (百度) - Wenxin Yiyan / ERNIE Bot (文心一言)
//! - Alibaba (阿里) - Tongyi Qianwen (通义千问)
//! - Xunfei (讯飞) - Spark (星火认知大模型)
//! - Zhipu (智谱) - ChatGLM

use super::{LlmClient, LlmConfig, LlmMessage, LlmRole, LlmStreamChunk};
use crate::domain::tools::ToolSchema;
use crate::errors::ApiError;
use async_trait::async_trait;
use futures::Stream;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

// ============================================================================
// Baidu Wenxin Yiyan (文心一言)
// ============================================================================

/// Baidu ERNIE Bot client
pub struct BaiduClient {
    config: LlmConfig,
    client: Client,
    access_token: Option<String>,
}

impl BaiduClient {
    pub fn new(config: LlmConfig) -> Self {
        let client = Client::builder()
            .connect_timeout(std::time::Duration::from_secs(60))
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            config,
            client,
            access_token: None,
        }
    }

    /// Get access token from API key and secret key
    async fn get_access_token(&self) -> Result<String, ApiError> {
        // If we already have a token, return it
        if let Some(token) = &self.access_token {
            return Ok(token.clone());
        }

        let api_key = &self.config.api_key;
        let secret_key = self
            .config
            .secret_key
            .as_ref()
            .ok_or_else(|| ApiError::Config {
                message: "Baidu requires secret_key".to_string(),
            })?;

        let url = format!(
            "https://aip.baidubce.com/oauth/2.0/token?grant_type=client_credentials&client_id={}&client_secret={}",
            api_key, secret_key
        );

        let response = self
            .client
            .post(&url)
            .send()
            .await
            .map_err(|e| ApiError::Network {
                message: e.to_string(),
            })?;

        let token_response: BaiduTokenResponse =
            response.json().await.map_err(|e| ApiError::Http {
                status: 500,
                message: format!("Failed to parse token response: {}", e),
            })?;

        Ok(token_response.access_token)
    }

    fn convert_messages(&self, messages: Vec<LlmMessage>) -> Vec<BaiduMessage> {
        messages
            .into_iter()
            .filter_map(|msg| {
                let role = match msg.role {
                    LlmRole::User => "user",
                    LlmRole::Assistant => "assistant",
                    _ => return None,
                };

                let content = msg
                    .content
                    .iter()
                    .filter_map(|c| c.as_text())
                    .collect::<Vec<_>>()
                    .join("\n");

                if content.is_empty() {
                    return None;
                }

                Some(BaiduMessage { role, content })
            })
            .collect()
    }
}

#[async_trait]
impl LlmClient for BaiduClient {
    async fn send_message_streaming(
        &self,
        messages: Vec<LlmMessage>,
        _tools: Vec<ToolSchema>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<LlmStreamChunk, ApiError>> + Send>>, ApiError>
    {
        let access_token = self.get_access_token().await?;
        let baidu_messages = self.convert_messages(messages);

        // For non-streaming, we'll simulate streaming by returning complete response
        // Baidu's streaming API is WebSocket-based and more complex
        let url = format!("{}?access_token={}", self.config.api_url(), access_token);

        let request = BaiduRequest {
            messages: baidu_messages,
            stream: true,
            temperature: self.config.temperature,
            max_output_tokens: Some(self.config.max_tokens),
        };

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| ApiError::Network {
                message: e.to_string(),
            })?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(ApiError::Http {
                status: status.as_u16(),
                message: text,
            });
        }

        let baidu_response: BaiduResponse = response.json().await.map_err(|e| ApiError::Http {
            status: 500,
            message: format!("Failed to parse response: {}", e),
        })?;

        if let Some(error) = baidu_response.error_code {
            return Err(ApiError::Server {
                message: format!(
                    "Baidu API error {}: {}",
                    error,
                    baidu_response.error_msg.unwrap_or_default()
                ),
            });
        }

        // Create a stream from the complete response (simulate streaming)
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<LlmStreamChunk, ApiError>>(10);
        let result_text = baidu_response.result.unwrap_or_default();

        tokio::spawn(async move {
            // Simulate streaming by breaking text into chunks
            let chunk_size = 10;
            for chunk in result_text.chars().collect::<Vec<_>>().chunks(chunk_size) {
                let text: String = chunk.iter().collect();
                if tx.send(Ok(LlmStreamChunk::Text(text))).await.is_err() {
                    return;
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
            }
            let _ = tx
                .send(Ok(LlmStreamChunk::Stop { stop_reason: None }))
                .await;
        });

        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Box::pin(stream))
    }

    async fn health_check(&self) -> Result<bool, ApiError> {
        match self.get_access_token().await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    fn config(&self) -> &LlmConfig {
        &self.config
    }
}

#[derive(Debug, Deserialize)]
struct BaiduTokenResponse {
    access_token: String,
}

#[derive(Debug, Serialize)]
struct BaiduRequest {
    messages: Vec<BaiduMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
}

#[derive(Debug, Serialize)]
struct BaiduMessage {
    role: &'static str,
    content: String,
}

#[derive(Debug, Deserialize)]
struct BaiduResponse {
    #[serde(rename = "error_code")]
    error_code: Option<i32>,
    #[serde(rename = "error_msg")]
    error_msg: Option<String>,
    result: Option<String>,
}

// ============================================================================
// Alibaba Tongyi Qianwen (通义千问)
// ============================================================================

/// Alibaba Dashscope client
pub struct AlibabaClient {
    config: LlmConfig,
    client: Client,
}

impl AlibabaClient {
    pub fn new(config: LlmConfig) -> Self {
        let client = Client::builder()
            .connect_timeout(std::time::Duration::from_secs(60))
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .expect("Failed to create HTTP client");

        Self { config, client }
    }

    fn headers(&self) -> reqwest::header::HeaderMap {
        use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};

        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", self.config.api_key)).unwrap(),
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers
    }

    fn convert_messages(&self, messages: Vec<LlmMessage>) -> Vec<AlibabaMessage> {
        messages
            .into_iter()
            .map(|msg| {
                let role = match msg.role {
                    LlmRole::System => "system",
                    LlmRole::User => "user",
                    LlmRole::Assistant => "assistant",
                    LlmRole::Tool => "user",
                };

                let content = msg
                    .content
                    .iter()
                    .filter_map(|c| c.as_text())
                    .collect::<Vec<_>>()
                    .join("\n");

                AlibabaMessage { role, content }
            })
            .collect()
    }
}

#[async_trait]
impl LlmClient for AlibabaClient {
    async fn send_message_streaming(
        &self,
        messages: Vec<LlmMessage>,
        _tools: Vec<ToolSchema>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<LlmStreamChunk, ApiError>> + Send>>, ApiError>
    {
        let alibaba_messages = self.convert_messages(messages);

        let request = AlibabaRequest {
            model: self.config.model.clone(),
            input: AlibabaInput {
                messages: alibaba_messages,
            },
            parameters: AlibabaParameters {
                max_tokens: Some(self.config.max_tokens),
                temperature: self.config.temperature,
                result_format: "message".to_string(),
            },
        };

        let response = self
            .client
            .post(&self.config.api_url())
            .headers(self.headers())
            .json(&request)
            .send()
            .await
            .map_err(|e| ApiError::Network {
                message: e.to_string(),
            })?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(ApiError::Http {
                status: status.as_u16(),
                message: text,
            });
        }

        let alibaba_response: AlibabaResponse =
            response.json().await.map_err(|e| ApiError::Http {
                status: 500,
                message: format!("Failed to parse response: {}", e),
            })?;

        if let Some(error) = alibaba_response.code {
            return Err(ApiError::Server {
                message: format!(
                    "Alibaba API error {}: {}",
                    error,
                    alibaba_response.message.unwrap_or_default()
                ),
            });
        }

        // Create stream from response
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<LlmStreamChunk, ApiError>>(10);
        let result_text = alibaba_response
            .output
            .and_then(|o| o.choices.first().cloned())
            .map(|c| c.message.content)
            .unwrap_or_default();

        tokio::spawn(async move {
            let chunk_size = 10;
            for chunk in result_text.chars().collect::<Vec<_>>().chunks(chunk_size) {
                let text: String = chunk.iter().collect();
                if tx.send(Ok(LlmStreamChunk::Text(text))).await.is_err() {
                    return;
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
            }
            let _ = tx
                .send(Ok(LlmStreamChunk::Stop { stop_reason: None }))
                .await;
        });

        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Box::pin(stream))
    }

    async fn health_check(&self) -> Result<bool, ApiError> {
        // Simple check - try to access API
        let response = self
            .client
            .head("https://dashscope.aliyuncs.com")
            .send()
            .await;

        match response {
            Ok(resp) => Ok(resp.status().is_success() || resp.status().as_u16() == 405), // 405 is ok, means auth required
            Err(_) => Ok(false),
        }
    }

    fn config(&self) -> &LlmConfig {
        &self.config
    }
}

#[derive(Debug, Serialize)]
struct AlibabaRequest {
    model: String,
    input: AlibabaInput,
    parameters: AlibabaParameters,
}

#[derive(Debug, Serialize)]
struct AlibabaInput {
    messages: Vec<AlibabaMessage>,
}

#[derive(Debug, Serialize)]
struct AlibabaMessage {
    role: &'static str,
    content: String,
}

#[derive(Debug, Serialize)]
struct AlibabaParameters {
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    result_format: String,
}

#[derive(Debug, Deserialize)]
struct AlibabaResponse {
    code: Option<String>,
    message: Option<String>,
    output: Option<AlibabaOutput>,
}

#[derive(Debug, Deserialize)]
struct AlibabaOutput {
    choices: Vec<AlibabaChoice>,
}

#[derive(Debug, Deserialize, Clone)]
struct AlibabaChoice {
    message: AlibabaChoiceMessage,
}

#[derive(Debug, Deserialize, Clone)]
struct AlibabaChoiceMessage {
    content: String,
}

// ============================================================================
// Xunfei Spark (讯飞星火)
// ============================================================================

/// Xunfei Spark client (WebSocket-based)
pub struct XunfeiClient {
    config: LlmConfig,
}

impl XunfeiClient {
    pub fn new(config: LlmConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl LlmClient for XunfeiClient {
    /// 使用讯飞星火新版 HTTP API（OpenAI 兼容）。
    ///
    /// 鉴权规则：
    /// - 若配置了 `secret_key`，则 Bearer token 为 `{api_key}:{secret_key}`（旧控制台凭据）
    /// - 否则直接使用 `api_key` 作为 APIPassword（新控制台，推荐）
    ///
    /// 接口地址：<https://spark-api-open.xf-yun.com/v1/chat/completions>
    async fn send_message_streaming(
        &self,
        messages: Vec<LlmMessage>,
        tools: Vec<ToolSchema>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<LlmStreamChunk, ApiError>> + Send>>, ApiError>
    {
        let bearer = match &self.config.secret_key {
            Some(sk) if !sk.is_empty() => format!("{}:{}", self.config.api_key, sk),
            _ => self.config.api_key.clone(),
        };

        let mut cfg = self.config.clone();
        cfg.api_key = bearer;
        if cfg.base_url.is_none() {
            cfg.base_url =
                Some("https://spark-api-open.xf-yun.com/v1/chat/completions".to_string());
        }

        let client = super::openai::OpenAiCompatibleClient::new(cfg);
        client.send_message_streaming(messages, tools).await
    }

    async fn health_check(&self) -> Result<bool, ApiError> {
        // 优先验证鉴权参数是否完整
        if self.config.api_key.is_empty() {
            return Ok(false);
        }
        Ok(true)
    }

    fn config(&self) -> &LlmConfig {
        &self.config
    }
}

// ============================================================================
// Google Gemini
// ============================================================================

/// Google Gemini 客户端。
///
/// 使用 Google 提供的 OpenAI 兼容端点：
/// `https://generativelanguage.googleapis.com/v1beta/openai/chat/completions`
///
/// 鉴权：`Authorization: Bearer {GEMINI_API_KEY}`
pub struct GoogleClient {
    inner: super::openai::OpenAiCompatibleClient,
    config: LlmConfig,
}

impl GoogleClient {
    pub fn new(mut config: LlmConfig) -> Self {
        if config.base_url.is_none() {
            config.base_url = Some(
                "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
                    .to_string(),
            );
        }
        let inner = super::openai::OpenAiCompatibleClient::new(config.clone());
        Self { inner, config }
    }
}

#[async_trait]
impl LlmClient for GoogleClient {
    async fn send_message_streaming(
        &self,
        messages: Vec<LlmMessage>,
        tools: Vec<ToolSchema>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<LlmStreamChunk, ApiError>> + Send>>, ApiError>
    {
        self.inner.send_message_streaming(messages, tools).await
    }

    async fn health_check(&self) -> Result<bool, ApiError> {
        self.inner.health_check().await
    }

    fn config(&self) -> &LlmConfig {
        &self.config
    }
}

// ============================================================================
// Zhipu AI ChatGLM (智谱)
// ============================================================================

/// Zhipu ChatGLM client (OpenAI-compatible)
pub struct ZhipuClient {
    inner: super::openai::OpenAiCompatibleClient,
}

impl ZhipuClient {
    pub fn new(config: LlmConfig) -> Self {
        // Zhipu uses OpenAI-compatible API
        let inner = super::openai::OpenAiCompatibleClient::new(config);
        Self { inner }
    }
}

#[async_trait]
impl LlmClient for ZhipuClient {
    async fn send_message_streaming(
        &self,
        messages: Vec<LlmMessage>,
        tools: Vec<ToolSchema>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<LlmStreamChunk, ApiError>> + Send>>, ApiError>
    {
        self.inner.send_message_streaming(messages, tools).await
    }

    async fn health_check(&self) -> Result<bool, ApiError> {
        self.inner.health_check().await
    }

    fn config(&self) -> &LlmConfig {
        self.inner.config()
    }
}
