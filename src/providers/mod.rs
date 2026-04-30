use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;

pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug)]
pub enum ProviderError {
    ApiError(String),
    NetworkError(String),
    AuthError(String),
    StreamError(String),
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderError::ApiError(msg) => write!(f, "api error: {}", msg),
            ProviderError::NetworkError(msg) => write!(f, "network error: {}", msg),
            ProviderError::AuthError(msg) => write!(f, "auth error: {}", msg),
            ProviderError::StreamError(msg) => write!(f, "stream error: {}", msg),
        }
    }
}

impl std::error::Error for ProviderError {}

#[async_trait]
pub trait ModelProvider: Send + Sync {
    async fn stream_completion(
        &self,
        messages: Vec<Message>,
    ) -> Result<Pin<Box<dyn Stream<Item = String> + Send>>, ProviderError>;
}

pub struct NoopProvider;

#[async_trait]
impl ModelProvider for NoopProvider {
    async fn stream_completion(
        &self,
        _messages: Vec<Message>,
    ) -> Result<Pin<Box<dyn Stream<Item = String> + Send>>, ProviderError> {
        Ok(Box::pin(futures::stream::empty()))
    }
}
