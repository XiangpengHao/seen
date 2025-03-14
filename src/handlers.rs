use crate::d1::{self, DocInfo};
use crate::models::Update;
use crate::utils::{chunk_and_summary_link, fetch_content, get_extension_from_content_type};
use crate::vector;
use uuid::Uuid;
use vector_lite::{ANNIndexOwned, Vector};
use worker::*;

/// Handle the webhook request from Telegram
pub async fn handle_webhook(mut req: Request, env: Env) -> Result<Response> {
    let update = req.json::<Update>().await?;
    crate::telegram::process_update(env, update).await?;
    Response::ok("OK")
}

/// Process and store a link
pub async fn insert_link(env: &Env, link: &str) -> Result<DocInfo> {
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

    // Process the content with Gemini API
    console_log!("Processing content with Gemini API from: {}", link);
    let processed_data = chunk_and_summary_link(env, &content, &content_type).await?;
    console_log!("Processed data: {:?}", processed_data);

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

    let mut embeddings = Vec::with_capacity(processed_data.chunks.len());
    for chunk_text in processed_data.chunks.iter() {
        let embedding = vector::generate_embeddings(env, chunk_text).await?;
        embeddings.push(embedding);
    }

    let mut vector_lite = vector::get_vector_lite(env).await?;

    for (i, embedding) in embeddings.iter().enumerate() {
        let vector_id = format!("{}-{}", link_id, i);
        vector_lite.insert(Vector::try_from(embedding.clone()).unwrap(), vector_id);
    }

    // TODO: how to make sure these steps are atomic?
    d1::save_to_bucket(env, &bucket_path, content.clone()).await?;
    d1::save_link_to_db(env, &row, &embeddings).await?;
    vector::save_vector_lite(env, &vector_lite).await?;

    Ok(row)
}

/// Prepare metadata for storage
fn get_bucket_path(content_type: &str, link_id: &str) -> String {
    let extension = get_extension_from_content_type(content_type);
    format!("content/{}.{}", link_id, extension)
}

/// Search links using vector similarity
/// Returns a list of links and their chunks
pub async fn search_links(
    env: Env,
    query: &str,
    search_from_cf: bool,
) -> Result<Vec<(DocInfo, f32)>> {
    console_log!("Searching for: {}", query);

    // Query the vector database to get vector IDs and scores
    let mut vector_results = if search_from_cf {
        vector::query_vectors_with_scores(&env, query, 20).await?
    } else {
        vector::query_vectors_with_scores_vector_lite(&env, query, 20).await?
    };

    if vector_results.is_empty() {
        return Ok(vec![]);
    }

    vector_results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    console_log!("Vector results: {:?}", vector_results);

    let mut sorted_docs = vec![];
    let mut doc_tracker = std::collections::HashSet::new();
    for (vector_id, score) in vector_results {
        let parts = vector_id.split("-").collect::<Vec<_>>();
        let document_id = parts[0..parts.len() - 1].join("-");
        if !doc_tracker.contains(&document_id) {
            doc_tracker.insert(document_id.clone());
            sorted_docs.push((document_id, score));
        }
        if sorted_docs.len() >= 5 {
            break;
        }
    }

    // Create a vector of futures for parallel execution
    let link_futures = sorted_docs.iter().take(5).map(|(doc_id, score)| {
        let env_clone = env.clone();
        let doc_id_clone = doc_id.clone();
        async move {
            match d1::get_link_by_id(&env_clone, &doc_id_clone).await {
                Ok(Some(link_info)) => Ok((link_info, *score)),
                Ok(None) => {
                    console_log!("Link not found, id: {}", doc_id_clone);
                    Err(Error::from(format!("Link {} not found", doc_id_clone)))
                }
                Err(e) => {
                    console_log!("Error fetching link {}: {:?}", doc_id_clone, e);
                    Err(e)
                }
            }
        }
    });

    let results = futures_util::future::join_all(link_futures).await;
    let return_val: Vec<(DocInfo, f32)> = results
        .into_iter()
        .collect::<Result<Vec<(DocInfo, f32)>>>()?;

    Ok(return_val)
}

/// Delete a link and all associated data
pub async fn delete_link(env: &Env, link: &str) -> Result<DocInfo> {
    console_log!("Deleting link: {}", link);

    let link_info = d1::delete_link_and_embedding_by_url(env, link).await?;

    let mut vector_lite = vector::get_vector_lite(env).await?;
    for i in 0..link_info.chunk_count {
        let vector_id = format!("{}-{}", link_info.id, i);
        vector_lite.delete_by_id(&vector_id);
    }

    d1::delete_from_bucket(env, &link_info.bucket_path).await?;

    vector::delete_vectors_by_prefix(env, &link_info.id, link_info.chunk_count).await?;
    vector::save_vector_lite(env, &vector_lite).await?;

    console_log!(
        "Successfully deleted link and all associated data: {}",
        link
    );

    Ok(link_info)
}
