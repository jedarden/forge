//! Claude API request and response types.
//!
//! This module contains the serde types for serializing requests to and
//! deserializing responses from the Anthropic Claude API.

use serde::{Deserialize, Serialize};

/// Request sent to the Claude API.
#[derive(Debug, Serialize)]
pub struct ApiRequest {
    pub model: String,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    pub system: String,
    pub messages: Vec<ApiMessage>,
    pub tools: Vec<ApiTool>,
}

/// Message in the API request.
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiMessage {
    pub role: String,
    pub content: String,
}

/// Tool definition in the API request.
#[derive(Debug, Serialize)]
pub struct ApiTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Response received from the Claude API.
#[derive(Debug, Deserialize, Clone)]
pub struct ApiResponse {
    #[allow(dead_code)]
    pub id: String,
    pub content: Vec<ContentBlock>,
    pub usage: ApiUsage,
}

/// Content block in the API response.
#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

/// Token usage information from the API response.
#[derive(Debug, Deserialize, Clone)]
pub struct ApiUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    #[serde(default)]
    pub cache_read_tokens: Option<u32>,
    #[serde(default)]
    pub cache_creation_tokens: Option<u32>,
}

/// Server-Sent Events (SSE) streaming event from Claude API.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum SseEvent {
    #[serde(rename = "message_start")]
    MessageStart { message: ApiMessageMeta },
    #[serde(rename = "message_delta")]
    MessageDelta { delta: MessageDelta, delta_usage: Option<DeltaUsage> },
    #[serde(rename = "message_stop")]
    MessageStop,
    #[serde(rename = "content_block_start")]
    ContentBlockStart { index: u32, content_block: Option<ContentBlockMeta> },
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta { index: u32, delta: ContentDelta },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: u32 },
    #[serde(rename = "error")]
    Error { error: ApiError },
}

/// Message metadata from streaming API.
#[derive(Debug, Deserialize)]
pub struct ApiMessageMeta {
    pub id: String,
    #[serde(default)]
    pub usage: Option<ApiUsage>,
}

/// Delta for message-level updates.
#[derive(Debug, Deserialize)]
pub struct MessageDelta {
    #[serde(default)]
    pub stop_reason: Option<String>,
}

/// Token usage delta from streaming API.
#[derive(Debug, Deserialize)]
pub struct DeltaUsage {
    #[serde(default)]
    pub output_tokens: Option<u32>,
}

/// Content block metadata.
#[derive(Debug, Deserialize)]
pub struct ContentBlockMeta {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub tool_use: Option<ToolUseMeta>,
}

/// Tool use metadata from streaming API.
#[derive(Debug, Deserialize)]
pub struct ToolUseMeta {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

/// Content delta from streaming API.
#[derive(Debug, Deserialize)]
pub struct ContentDelta {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub tool_use: Option<ToolUseDelta>,
}

/// Tool use delta from streaming API.
#[derive(Debug, Deserialize)]
pub struct ToolUseDelta {
    #[serde(default)]
    pub input: Option<serde_json::Value>,
}

/// Error from streaming API.
#[derive(Debug, Deserialize)]
pub struct ApiError {
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub type_: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_request_serialization() {
        let request = ApiRequest {
            model: "claude-sonnet-4-5".to_string(),
            max_tokens: 1000,
            temperature: Some(0.5),
            system: "You are a helpful assistant.".to_string(),
            messages: vec![ApiMessage {
                role: "user".to_string(),
                content: "Hello!".to_string(),
            }],
            tools: vec![ApiTool {
                name: "test_tool".to_string(),
                description: "A test tool".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
            }],
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("claude-sonnet-4-5"));
        assert!(json.contains("Hello!"));
        assert!(json.contains("test_tool"));

        // Verify temperature is serialized
        assert!(json.contains("0.5"));
    }

    #[test]
    fn test_api_request_without_temperature() {
        let request = ApiRequest {
            model: "claude-sonnet-4-5".to_string(),
            max_tokens: 1000,
            temperature: None,
            system: "You are a helpful assistant.".to_string(),
            messages: vec![],
            tools: vec![],
        };

        let json = serde_json::to_string(&request).unwrap();
        // Temperature should not be in the JSON when None
        assert!(!json.contains("temperature"));
    }

    #[test]
    fn test_api_response_parsing() {
        let json = r#"{
            "id": "msg_123",
            "content": [
                {"type": "text", "text": "Hello there!"},
                {
                    "type": "tool_use",
                    "id": "tool_123",
                    "name": "get_weather",
                    "input": {"location": "San Francisco"}
                }
            ],
            "usage": {
                "input_tokens": 10,
                "output_tokens": 20,
                "cache_read_tokens": 5,
                "cache_creation_tokens": 2
            }
        }"#;

        let response: ApiResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.id, "msg_123");
        assert_eq!(response.content.len(), 2);
        assert_eq!(response.usage.input_tokens, 10);
        assert_eq!(response.usage.output_tokens, 20);
        assert_eq!(response.usage.cache_read_tokens, Some(5));
        assert_eq!(response.usage.cache_creation_tokens, Some(2));
    }

    #[test]
    fn test_content_block_text() {
        let json = r#"{"type": "text", "text": "Hello, world!"}"#;
        let block: ContentBlock = serde_json::from_str(json).unwrap();

        match block {
            ContentBlock::Text { text } => {
                assert_eq!(text, "Hello, world!");
            }
            ContentBlock::ToolUse { .. } => panic!("Expected Text block"),
        }
    }

    #[test]
    fn test_content_block_tool_use() {
        let json = r#"{
            "type": "tool_use",
            "id": "tool_abc",
            "name": "calculate",
            "input": {"x": 42, "y": 10}
        }"#;
        let block: ContentBlock = serde_json::from_str(json).unwrap();

        match block {
            ContentBlock::Text { .. } => panic!("Expected ToolUse block"),
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "tool_abc");
                assert_eq!(name, "calculate");
                assert_eq!(input["x"], 42);
                assert_eq!(input["y"], 10);
            }
        }
    }

    #[test]
    fn test_api_usage_defaults() {
        let json = r#"{
            "input_tokens": 100,
            "output_tokens": 50
        }"#;
        let usage: ApiUsage = serde_json::from_str(json).unwrap();

        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_tokens, None);
        assert_eq!(usage.cache_creation_tokens, None);
    }

    #[test]
    fn test_api_message_roundtrip() {
        let msg = ApiMessage {
            role: "user".to_string(),
            content: "Test message".to_string(),
        };

        let json = serde_json::to_string(&msg).unwrap();
        let parsed: ApiMessage = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.role, "user");
        assert_eq!(parsed.content, "Test message");
    }

    #[test]
    fn test_api_tool_serialization() {
        let tool = ApiTool {
            name: "search".to_string(),
            description: "Search the web".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                }
            }),
        };

        let json = serde_json::to_string(&tool).unwrap();
        assert!(json.contains("search"));
        assert!(json.contains("Search the web"));
        assert!(json.contains("query"));
    }
}
