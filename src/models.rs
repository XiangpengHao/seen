use serde::{Deserialize, Serialize};

// ===== Telegram API Models =====
#[derive(Deserialize, Serialize)]
pub struct Update {
    pub update_id: i64,
    #[serde(default)]
    pub message: Option<Message>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Message {
    pub message_id: i64,
    pub chat: Chat,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub from: Option<User>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Chat {
    pub id: i64,
    #[serde(default)]
    pub first_name: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub type_field: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct User {
    pub id: i64,
    #[serde(default)]
    pub first_name: Option<String>,
    #[serde(default)]
    pub last_name: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
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

#[derive(Serialize, Deserialize, Debug)]
pub struct VectorMetadata {
    pub chunk_id: u64,
    pub document_id: String,
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
