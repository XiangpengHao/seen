use crate::models::{LinkInfo, Update, VectorMetadata};
use crate::storage;
use crate::utils::{
    chunk_and_summary_link, fetch_content, get_extension_from_content_type, ProcessedLinkData,
};
use crate::vector;
use futures_util;
use serde_json::json;
use uuid::Uuid;
use worker::*;

/// Handle the webhook request from Telegram
pub async fn handle_webhook(mut req: Request, env: Env) -> Result<Response> {
    let update = req.json::<Update>().await?;
    crate::telegram::process_update(env, update).await?;
    Response::ok("OK")
}

/// Handle a link processing request
pub async fn handle_link_request(req: Request, env: Env) -> Result<Response> {
    // Check if it's a GET request
    if req.method() != Method::Get {
        return Response::error("Method Not Allowed", 405);
    }

    // Parse the URL to extract 'url' query parameter
    let url = req.url()?;
    let url_param = url
        .query_pairs()
        .find(|(key, _)| key == "url")
        .map(|(_, value)| value.to_string());

    // Return error if no URL parameter is provided
    let url_to_save = match url_param {
        Some(u) => u,
        None => return Response::error("Missing 'url' parameter", 400),
    };

    // Validate URL format
    if !url_to_save.starts_with("http://") && !url_to_save.starts_with("https://") {
        return Response::error(
            "Invalid URL format, must start with http:// or https://",
            400,
        );
    }

    // Use the handle_link function to process the URL
    match handle_link(&env, &url_to_save).await {
        Ok(link_info) => {
            // Create JSON response with the result
            let response_data = json!({
                "success": true,
                "url": url_to_save,
                "content_type": link_info.content_type,
                "size": crate::utils::format_size(link_info.size),
                "timestamp": link_info.timestamp,
                "bucket_path": link_info.bucket_path
            });

            // Return a JSON response
            let mut headers = Headers::new();
            headers.set("Content-Type", "application/json")?;

            Response::from_json(&response_data).map(|resp| resp.with_headers(headers))
        }
        Err(e) => {
            // Create error response
            let error_data = json!({
                "success": false,
                "error": e.to_string(),
                "url": url_to_save
            });

            Response::from_json(&error_data).map(|resp| resp.with_status(500))
        }
    }
}

/// Process and store a link
pub async fn handle_link(env: &Env, link: &str) -> Result<LinkInfo> {
    // First check if link already exists in database
    if let Ok(existing_link) = storage::find_link_by_url(env, link).await {
        let kv = env.kv("SEEN_KV")?;
        let processed_data = kv
            .get(&existing_link.id)
            .json::<ProcessedLinkData>()
            .await?
            .ok_or_else(|| Error::from("Processed data not found in KV"))?;
        return Ok(LinkInfo {
            content_type: existing_link.content_type,
            type_emoji: existing_link.type_emoji.to_string(),
            size: existing_link.size,
            timestamp: existing_link.created_at,
            bucket_path: existing_link.bucket_path,
            num_chunks: processed_data.chunks.len(),
            summary: processed_data.summary,
        });
    }

    let link_id = Uuid::new_v4().to_string();
    let current_time = js_sys::Date::new_0().to_iso_string().as_string().unwrap();
    let env_clone = env.clone(); // Clone for use in the parallel tasks

    // Run content fetching and Gemini processing in parallel
    let (content_result, processed_data) = futures_util::join!(
        // Task 1: Fetch and save content
        async {
            // Fetch the content
            let (content, content_type) = fetch_content(link).await?;
            let (bucket_path, type_emoji, content_size) =
                prepare_storage_metadata(&content_type, &link_id, content.len());

            // Save content to bucket
            storage::save_to_bucket(env, &bucket_path, content).await?;

            Ok::<_, Error>((
                content_type.clone(),
                bucket_path.clone(),
                type_emoji,
                content_size,
            ))
        },
        // Task 2: Process the link with Gemini API (doesn't need content)
        async {
            console_log!("Processing link with Gemini API: {}", link);
            chunk_and_summary_link(&env_clone, link).await
        }
    );

    // Unwrap results from both parallel tasks
    let (content_type, bucket_path, type_emoji, content_size) = content_result?;
    let processed_data = processed_data?;

    // Store link info in database
    storage::save_link_to_db(
        env,
        &link_id,
        link,
        &bucket_path,
        &content_type,
        content_size,
        &processed_data.title,
        &processed_data.summary,
    )
    .await?;

    // Process each chunk and generate embeddings
    for (i, chunk_text) in processed_data.chunks.iter().enumerate() {
        let embeddings = vector::generate_embeddings(env, chunk_text).await?;

        let vector_metadata = VectorMetadata {
            url: link.to_string(),
            chunk_id: i as u64,
        };

        vector::insert_vector(env, &link_id, vector_metadata, embeddings).await?;
    }

    let kv = env.kv("SEEN_KV")?;
    kv.put(&link_id, &processed_data)?.execute().await?;
    console_log!("Stored processed data in KV with key: {}", link_id);

    // Create information structure to return
    let link_info = LinkInfo {
        content_type: content_type.clone(),
        type_emoji: type_emoji.to_string(),
        size: content_size,
        timestamp: current_time.clone(),
        bucket_path: bucket_path.clone(),
        num_chunks: processed_data.chunks.len(),
        summary: processed_data.summary,
    };

    Ok(link_info)
}

/// Prepare metadata for storage
fn prepare_storage_metadata(
    content_type: &str,
    link_id: &str,
    content_size: usize,
) -> (String, &'static str, usize) {
    let extension = get_extension_from_content_type(content_type);
    let type_emoji = storage::format_type_emoji(content_type);
    let bucket_path = format!("content/{}.{}", link_id, extension);

    (bucket_path, type_emoji, content_size)
}

/// Search links using vector similarity
pub async fn search_links(env: Env, query: &str) -> Result<String> {
    console_log!("Searching for: {}", query);

    // Query the vector database to get vector IDs and scores
    let vector_results = vector::query_vectors_with_scores(&env, query, 20).await?;

    if vector_results.is_empty() {
        return Ok("No results found for your query. Try a different search term.".to_string());
    }

    // Group results by document ID to collect all chunks from the same document
    // Map of document_id -> Vec<(score, chunk_id)>
    let mut doc_matches: std::collections::HashMap<String, Vec<(f32, u64)>> =
        std::collections::HashMap::new();

    // Also track the best score for each document for sorting
    let mut doc_best_scores: std::collections::HashMap<String, f32> =
        std::collections::HashMap::new();

    for (vector_id, score, metadata) in vector_results {
        let document_id = vector_id;
        let chunk_id = metadata.chunk_id;

        // Add this chunk to the document's matches
        doc_matches
            .entry(document_id.clone())
            .or_insert_with(Vec::new)
            .push((score, chunk_id));

        // Update the document's best score if this is higher
        let current_best = doc_best_scores.entry(document_id).or_insert(0.0);
        if score > *current_best {
            *current_best = score;
        }
    }

    // Build response with matching links (deduplicated by document)
    let mut response = format!("🔍 Search results for '{}'\n\n", query);

    // Sort documents by their best score
    let mut sorted_docs: Vec<_> = doc_best_scores.into_iter().collect();
    sorted_docs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Show only top 5 documents
    for (i, (doc_id, _)) in sorted_docs.iter().take(5).enumerate() {
        match storage::get_link_by_id(&env, doc_id).await {
            Ok(link_info) => {
                let title_display = link_info.title;

                // Sort the chunks by score (highest first)
                let mut chunks = doc_matches.get(doc_id).unwrap().clone();
                chunks.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

                // Format the list of chunks
                let chunk_list = chunks
                    .iter()
                    .map(|(_, chunk_id)| format!("{}", chunk_id + 1)) // +1 for 1-indexed display
                    .collect::<Vec<_>>()
                    .join(", ");

                // Format each result with its details and all matching chunks
                response.push_str(&format!(
                    "{}. {} {} \n(chunks: {})\n{}\n{}\n\n",
                    i + 1,
                    link_info.type_emoji,
                    title_display,
                    chunk_list,
                    link_info.url,
                    link_info.summary
                ));
            }
            Err(e) => {
                console_error!("Error fetching link info for ID {}: {}", doc_id, e);
                response.push_str(&format!(
                    "{}. ⚠️ Result found but details unavailable (ID: {}, error: {})\n\n",
                    i + 1,
                    doc_id,
                    e
                ));
            }
        }
    }

    Ok(response)
}
