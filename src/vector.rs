use crate::models::{
    EmbeddingRequest, EmbeddingResponse, VectorMetadata, VectorQueryRequest, VectorQueryResponse,
};
use serde_json::json;
use worker::*;

// Constants for Workers AI
const CF_ACCOUNT_ID: &str = "CF_ACCOUNT_ID";
const CF_API_TOKEN: &str = "CF_API_TOKEN";
const VECTORIZE_INDEX_NAME: &str = "seen-index";
const WORKERS_AI_API_URL: &str =
    "https://api.cloudflare.com/client/v4/accounts/{account_id}/ai/run/@cf/baai/bge-base-en-v1.5";

/// Generates embeddings for text using Workers AI
pub async fn generate_embeddings(env: &Env, text: &str) -> Result<Vec<f32>> {
    let account_id = env.secret(CF_ACCOUNT_ID)?.to_string();
    let api_token = env.secret(CF_API_TOKEN)?.to_string();

    let url = WORKERS_AI_API_URL.replace("{account_id}", &account_id);

    let embedding_req = EmbeddingRequest {
        text: vec![text.to_string()],
    };

    let mut headers = Headers::new();
    headers.set("Authorization", &format!("Bearer {}", api_token))?;
    headers.set("Content-Type", "application/json")?;

    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(wasm_bindgen::JsValue::from_str(
            &serde_json::to_string(&embedding_req)?,
        )));

    let request = Request::new_with_init(&url, &init)?;
    let mut response = Fetch::Request(request).send().await?;

    if response.status_code() != 200 {
        let error_text = response.text().await?;
        console_error!("Failed to generate embeddings: {}", error_text);
        return Err(Error::from(format!(
            "Failed to generate embeddings, error: {}",
            error_text
        )));
    }

    let embedding_response: EmbeddingResponse = response.json().await?;

    if !embedding_response.success || embedding_response.result.data.is_empty() {
        return Err(Error::from("Failed to generate embeddings: empty response"));
    }

    Ok(embedding_response.result.data[0].clone())
}

/// Inserts a vector into the Vectorize index
pub async fn insert_vector(
    env: &Env,
    id: &str,
    metadata: VectorMetadata,
    values: Vec<f32>,
) -> Result<()> {
    let account_id = env.secret(CF_ACCOUNT_ID)?.to_string();
    let api_token = env.secret(CF_API_TOKEN)?.to_string();

    let url_endpoint = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/vectorize/v2/indexes/{}/insert",
        account_id, VECTORIZE_INDEX_NAME
    );

    // Create a vector object in JSON format
    let vector_obj = json!({
        "id": id.to_string(),
        "values": values,
        "metadata": metadata
    });

    let ndjson = serde_json::to_string(&vector_obj)?;

    let mut headers = Headers::new();
    headers.set("Authorization", &format!("Bearer {}", api_token))?;
    headers.set("Content-Type", "application/x-ndjson")?;

    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(wasm_bindgen::JsValue::from_str(&ndjson)));

    let request = Request::new_with_init(&url_endpoint, &init)?;
    let mut response = Fetch::Request(request).send().await?;

    if response.status_code() != 200 {
        let error_text = response.text().await?;
        console_error!("Failed to insert vector: {}", error_text);
        return Err(Error::from("Failed to insert vector"));
    }

    console_log!("Vector inserted successfully for link ID: {}", id);
    Ok(())
}

/// Queries the Vectorize index for similar vectors and returns IDs, scores, and metadata
pub async fn query_vectors_with_scores(
    env: &Env,
    query_text: &str,
    top_k: usize,
) -> Result<Vec<(String, f32, VectorMetadata)>> {
    let account_id = env.secret(CF_ACCOUNT_ID)?.to_string();
    let api_token = env.secret(CF_API_TOKEN)?.to_string();

    // Generate embedding for the query text
    let query_vector = generate_embeddings(env, query_text).await?;

    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/vectorize/v2/indexes/{}/query",
        account_id, VECTORIZE_INDEX_NAME
    );

    let query_req = VectorQueryRequest {
        vector: query_vector,
        top_k,
        return_metadata: "all".to_string(),
    };

    let mut headers = Headers::new();
    headers.set("Authorization", &format!("Bearer {}", api_token))?;
    headers.set("Content-Type", "application/json")?;

    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(wasm_bindgen::JsValue::from_str(
            &serde_json::to_string(&query_req)?,
        )));

    let request = Request::new_with_init(&url, &init)?;
    let mut response = Fetch::Request(request).send().await?;

    if response.status_code() != 200 {
        let error_text = response.text().await?;
        console_error!("Failed to query vectors: {}", error_text);
        return Err(Error::from("Failed to query vectors"));
    }

    let query_response: VectorQueryResponse = response.json().await?;

    if !query_response.success {
        return Err(Error::from(
            "Failed to query vectors: unsuccessful response",
        ));
    }

    // Return vector IDs with scores and metadata
    Ok(query_response
        .result
        .matches
        .into_iter()
        .map(|m| (m.id, m.score, m.metadata))
        .collect())
}

/// Deletes vectors from the Vectorize index with IDs matching the document ID
pub async fn delete_vectors_by_prefix(
    env: &Env,
    id_prefix: &str,
    chunk_count: usize,
) -> Result<()> {
    let account_id = env.secret(CF_ACCOUNT_ID)?.to_string();
    let api_token = env.secret(CF_API_TOKEN)?.to_string();

    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/vectorize/v2/indexes/{}/delete_by_ids",
        account_id, VECTORIZE_INDEX_NAME
    );

    // Construct vector IDs based on the document ID and chunk count
    let mut vector_ids = Vec::with_capacity(chunk_count);
    for i in 0..chunk_count {
        vector_ids.push(format!("{}-{}", id_prefix, i));
    }

    let delete_payload = json!({
        "ids": vector_ids
    });

    let mut headers = Headers::new();
    headers.set("Authorization", &format!("Bearer {}", api_token))?;
    headers.set("Content-Type", "application/json")?;

    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(wasm_bindgen::JsValue::from_str(
            &serde_json::to_string(&delete_payload)?,
        )));

    let request = Request::new_with_init(&url, &init)?;
    let mut response = Fetch::Request(request).send().await?;

    if response.status_code() != 200 {
        let error_text = response.text().await?;
        console_error!("Failed to delete vectors: {}", error_text);
        return Err(Error::from("Failed to delete vectors"));
    }

    // Parse the response to check if success is true
    let response_data: serde_json::Value = response.json().await?;
    if !response_data
        .get("success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        console_error!("Vector deletion reported failure: {:?}", response_data);
        return Err(Error::from(format!(
            "Vector deletion reported failure, response: {}",
            response_data
        )));
    }

    console_log!(
        "Vectors deleted successfully for document ID: {}",
        id_prefix
    );
    Ok(())
}
