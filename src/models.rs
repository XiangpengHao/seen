use serde::{Deserialize, Serialize};

// ===== Telegram API Models =====
#[derive(Deserialize, Serialize)]
pub struct Update {
    pub update_id: i64,
    #[serde(default)]
    pub message: Option<Message>,
}

#[derive(Deserialize, Serialize)]
pub struct Message {
    pub message_id: i64,
    pub chat: Chat,
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub struct Chat {
    pub id: i64,
}

// ===== Vector Database Models =====
#[derive(Serialize)]
pub struct EmbeddingRequest {
    pub text: Vec<String>,
}

#[derive(Deserialize, Debug)]
pub struct EmbeddingResponse {
    pub result: EmbeddingResult,
    pub success: bool,
}

#[derive(Deserialize, Debug, Serialize)]
pub struct EmbeddingResult {
    pub shape: Vec<usize>,
    pub data: Vec<Vec<f32>>,
}

#[derive(Serialize, Deserialize)]
pub struct VectorMetadata {
    pub url: String,
    pub title: Option<String>,
    pub bucket_path: String,
    pub content_type: String,
}

#[derive(Serialize)]
pub struct VectorQueryRequest {
    pub vector: Vec<f32>,
    pub top_k: usize,
}

#[derive(Deserialize)]
pub struct VectorQueryResponse {
    pub result: VectorQueryResult,
    pub success: bool,
}

#[derive(Deserialize)]
pub struct VectorQueryResult {
    #[allow(dead_code)]
    pub count: usize,
    pub matches: Vec<VectorMatch>,
}

#[derive(Deserialize)]
pub struct VectorMatch {
    pub id: String,
    #[allow(dead_code)]
    pub score: f32,
}

// ===== Link Models =====
#[derive(Debug)]
pub struct LinkInfo {
    pub content_type: String,
    pub type_emoji: String,
    pub size: usize,
    pub timestamp: String,
    pub bucket_path: String,
}

#[derive(Debug)]
pub struct LinkInfoWithURL {
    pub url: String,
    pub title: Option<String>,
    #[allow(dead_code)]
    pub content_type: String,
    pub type_emoji: String,
    #[allow(dead_code)]
    pub bucket_path: String,
}

/// Content metadata for processing
pub struct ContentMetadata<'a> {
    pub link_id: &'a str,
    pub url: &'a str,
    pub content_type: &'a str,
    pub bucket_path: &'a str,
    pub title: Option<String>,
}
