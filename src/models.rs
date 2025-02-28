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
    pub chunk_id: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VectorQueryRequest {
    pub vector: Vec<f32>,
    pub top_k: usize,
    pub return_metadata: String,
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
    pub metadata: VectorMetadata,
}

// ===== Link Models =====
#[derive(Debug)]
pub struct LinkInfo {
    pub content_type: String,
    pub type_emoji: String,
    pub size: usize,
    pub timestamp: String,
    pub bucket_path: String,
    pub num_chunks: usize,
    pub summary: String,
}

#[derive(Debug)]
pub struct LinkInfoWithURL {
    pub url: String,
    pub id: String,
    pub title: String,
    pub content_type: String,
    pub type_emoji: String,
    pub bucket_path: String,
    pub summary: String,
    pub size: usize,
    pub created_at: String,
}
