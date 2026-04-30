use crate::providers::{Message, ModelProvider};

pub struct AgentInput {
    pub task: String,
    pub conversation_history: Vec<Message>,
}

pub struct AgentOutput {
    pub response: String,
}

#[derive(Debug)]
pub enum AgentError {}

impl std::fmt::Display for AgentError {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {}
    }
}

impl std::error::Error for AgentError {}

pub trait Agent {
    async fn run(
        &self,
        input: AgentInput,
        provider: &dyn ModelProvider,
    ) -> Result<AgentOutput, AgentError>;
}

pub struct CoderAgent;

impl CoderAgent {
    pub fn new() -> Self {
        CoderAgent
    }
}

impl Agent for CoderAgent {
    async fn run(
        &self,
        _input: AgentInput,
        _provider: &dyn ModelProvider,
    ) -> Result<AgentOutput, AgentError> {
        Ok(AgentOutput {
            response: "Coder agent placeholder".to_string(),
        })
    }
}
