#![allow(dead_code)]
#![allow(clippy::enum_variant_names)]

use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub enum Role {
    #[serde(rename = "system")]
    System,
    #[serde(rename = "user")]
    User,
    #[serde(rename = "assistant")]
    Assistant,
    Other(String),
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::System => f.write_str("system"),
            Role::User => f.write_str("user"),
            Role::Assistant => f.write_str("assistant"),
            Role::Other(s) => f.write_str(s.as_str()),
        }
    }
}

impl<'de> Deserialize<'de> for Role {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "system" => Role::System,
            "user" => Role::User,
            "assistant" => Role::Assistant,
            _ => Role::Other(s),
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
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
