use crate::d1::{self, DocInfo};
use crate::models::{Update, VectorMetadata};
use crate::utils::{chunk_and_summary_link, fetch_content, get_extension_from_content_type};
use crate::vector;
use uuid::Uuid;
use worker::*;

/// Handle the webhook request from Telegram
pub async fn handle_webhook(mut req: Request, env: Env) -> Result<Response> {
    let update = req.json::<Update>().await?;
    crate::telegram::process_update(env, update).await?;
    Response::ok("OK")
}

/// Process and store a link
pub async fn handle_link(env: &Env, link: &str) -> Result<DocInfo> {
    if let Ok(existing_link) = d1::find_link_by_url(env, link).await {
        return Ok(existing_link);
    }

    let link_id = Uuid::new_v4().to_string();
    let current_time = js_sys::Date::new_0().to_iso_string().as_string().unwrap();

    // Download content first
    console_log!("Fetching content from link: {}", link);
    let (content, content_type) = fetch_content(link).await?;
    let bucket_path = get_bucket_path(&content_type, &link_id);
    let content_size = content.len();

    // Save content to bucket
    d1::save_to_bucket(env, &bucket_path, content.clone()).await?;

    // Process the content with Gemini API
    console_log!("Processing content with Gemini API from: {}", link);
    let processed_data = chunk_and_summary_link(env, &content, &content_type).await?;

    let row = DocInfo {
        id: link_id.clone(),
        url: link.to_string(),
        created_at: current_time.clone(),
        bucket_path: bucket_path.clone(),
        content_type: content_type.clone(),
        size: content_size,
        title: processed_data.title.clone(),
        summary: processed_data.summary.clone(),
        chunk_count: processed_data.chunks.len(),
    };

    d1::save_link_to_db(env, &row).await?;

    // Process each chunk and generate embeddings
    for (i, chunk_text) in processed_data.chunks.iter().enumerate() {
        let embeddings = vector::generate_embeddings(env, chunk_text).await?;

        let vector_id = format!("{}-{}", link_id, i);
        let vector_metadata = VectorMetadata {
            document_id: link_id.clone(),
            chunk_id: i as u64,
        };

        vector::insert_vector(env, &vector_id, vector_metadata, embeddings).await?;
    }

    let kv = env.kv("SEEN_KV")?;
    kv.put(&link_id, &processed_data)?.execute().await?;
    console_log!("Stored processed data in KV with key: {}", link_id);

    Ok(row)
}

/// Prepare metadata for storage
fn get_bucket_path(content_type: &str, link_id: &str) -> String {
    let extension = get_extension_from_content_type(content_type);
    format!("content/{}.{}", link_id, extension)
}

/// Search links using vector similarity
/// Returns a list of links and their chunks
pub async fn search_links(env: Env, query: &str) -> Result<Vec<(DocInfo, Vec<u64>)>> {
    console_log!("Searching for: {}", query);

    // Query the vector database to get vector IDs and scores
    let vector_results = vector::query_vectors_with_scores(&env, query, 20).await?;

    if vector_results.is_empty() {
        return Ok(vec![]);
    }

    // Group results by document ID to collect all chunks from the same document
    // Map of document_id -> Vec<(score, chunk_id)>
    let mut doc_matches: std::collections::HashMap<String, Vec<(f32, u64)>> =
        std::collections::HashMap::new();

    // Also track the best score for each document for sorting
    let mut doc_best_scores: std::collections::HashMap<String, f32> =
        std::collections::HashMap::new();

    for (_vector_id, score, metadata) in vector_results {
        doc_matches
            .entry(metadata.document_id.clone())
            .or_default()
            .push((score, metadata.chunk_id));

        // Update the document's best score if this is higher
        let current_best = doc_best_scores.entry(metadata.document_id).or_insert(0.0);
        if score > *current_best {
            *current_best = score;
        }
    }

    // Sort documents by their best score
    let mut sorted_docs: Vec<_> = doc_best_scores.into_iter().collect();
    sorted_docs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut return_val = Vec::new();

    for (doc_id, _) in sorted_docs.iter().take(5) {
        let link_info = d1::get_link_by_id(&env, doc_id).await?;

        // Sort the chunks by score (highest first)
        let mut chunks = doc_matches.get(doc_id).unwrap().clone();
        chunks.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let chunk_list = chunks
            .iter()
            .map(|(_, chunk_id)| *chunk_id) // +1 for 1-indexed display
            .collect::<Vec<_>>();
        return_val.push((link_info, chunk_list));
    }

    Ok(return_val)
}
