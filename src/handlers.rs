use std::future::Future;
use std::pin::Pin;

use crate::models::{LinkInfo, Update};
use crate::storage;
use crate::utils::{
    extract_text_from_html, extract_text_from_pdf_with_gemini, extract_title_from_html,
    fetch_content, get_extension_from_content_type,
};
use crate::vector;
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
    match handle_link(&env, &url_to_save, dummy_logger).await {
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

fn dummy_logger(_text: &str) -> Pin<Box<dyn Future<Output = Result<()>>>> {
    Box::pin(async move { Ok(()) })
}

/// Process and store a link
pub async fn handle_link(
    env: &Env,
    link: &str,
    logger: impl Fn(&str) -> Pin<Box<dyn Future<Output = Result<()>>>>,
) -> Result<LinkInfo> {
    let link_id = Uuid::new_v4().to_string();

    // Fetch the content
    let (content, content_type) = fetch_content(link).await?;
    logger(&format!(
        "Fetched link: {}, length: {}",
        link,
        content.len()
    ))
    .await?;

    // Process metadata and prepare storage
    let (bucket_path, type_emoji, content_size) =
        prepare_storage_metadata(&content_type, &link_id, content.len());
    let current_time = js_sys::Date::new_0().to_iso_string().as_string().unwrap();

    // Save content to bucket
    storage::save_to_bucket(env, &bucket_path, content.clone()).await?;
    logger(&format!("Saved content to bucket: {}", bucket_path)).await?;

    // Process content based on type and generate embeddings
    let (extracted_text, title) = document_to_string(env, &content_type, link, &content).await?;
    logger(&format!("Extracted text length: {}", extracted_text.len())).await?;

    generate_and_store_embeddings(env, &extracted_text, &link_id, link, &bucket_path, &logger)
        .await?;
    logger(&format!("Generated embeddings for: {}", link)).await?;

    // Store link info in database
    storage::save_link_to_db(
        env,
        link,
        &bucket_path,
        &content_type,
        content_size,
        Some(&title),
    )
    .await?;

    logger(&format!("Saved link info to database: {}", link)).await?;

    // Create information structure to return
    let link_info = LinkInfo {
        content_type: content_type.clone(),
        type_emoji: type_emoji.to_string(),
        size: content_size,
        timestamp: current_time.clone(),
        bucket_path: bucket_path.clone(),
    };

    Ok(link_info)
}

/// Prepare metadata for storage
fn prepare_storage_metadata<'a>(
    content_type: &'a str,
    link_id: &str,
    content_size: usize,
) -> (String, &'a str, usize) {
    let extension = get_extension_from_content_type(content_type);
    let type_emoji = storage::format_type_emoji(content_type);
    let bucket_path = format!("content/{}.{}", link_id, extension);

    (bucket_path, type_emoji, content_size)
}

/// Process content based on its type and generate embeddings
async fn document_to_string(
    env: &Env,
    content_type: &str,
    url: &str,
    content: &[u8],
) -> Result<(String, String)> {
    let (extracted_text, title) = if content_type.starts_with("text/html") {
        // Process HTML content
        process_html_content(url, content).await?
    } else if content_type.starts_with("application/pdf") {
        // Process PDF content
        process_pdf_content(env, url, content).await?
    } else {
        return Err(Error::from("Unsupported content type"));
    };

    Ok((extracted_text, title))
}

/// Process HTML content and generate embeddings
async fn process_html_content(url: &str, raw_html: &[u8]) -> Result<(String, String)> {
    let html_content = String::from_utf8(raw_html.to_vec()).unwrap_or_default();
    let title = match extract_title_from_html(&html_content) {
        Some(title) => title,
        None => {
            // Extract title from URL if not found in HTML
            let url_parts: Vec<&str> = url.split('/').collect();
            let filename = url_parts.last().unwrap_or(&"Untitled Document");

            // Remove query parameters and file extensions if present
            let clean_filename = if filename.contains('?') {
                filename.split('?').next().unwrap_or("Untitled Document")
            } else {
                filename
            };

            // Remove file extension and replace hyphens/underscores with spaces
            let title_from_url = clean_filename
                .split('.')
                .next()
                .unwrap_or("Untitled Document")
                .replace('-', " ")
                .replace('_', " ");

            // Capitalize first letter of each word for better readability
            title_from_url
                .split_whitespace()
                .map(|word| {
                    if let Some(first_char) = word.chars().next() {
                        let first_upper = first_char.to_uppercase().collect::<String>();
                        first_upper + &word[first_char.len_utf8()..]
                    } else {
                        word.to_string()
                    }
                })
                .collect::<Vec<String>>()
                .join(" ")
        }
    };
    let extracted_text = extract_text_from_html(&html_content);
    Ok((extracted_text, title))
}

/// Process PDF content and generate embeddings
async fn process_pdf_content(env: &Env, url: &str, content: &[u8]) -> Result<(String, String)> {
    console_log!("Processing PDF content with Gemini API...");

    // Extract text from PDF
    let extracted_text = extract_text_from_pdf_with_gemini(env, content).await?;
    let title = url
        .split('/')
        .last()
        .unwrap_or("PDF Document")
        .replace(".pdf", "")
        .to_string();

    Ok((extracted_text, title))
}

/// Generate embeddings and store them in the vector database
async fn generate_and_store_embeddings(
    env: &Env,
    text: &str,
    link_id: &str,
    url: &str,
    bucket_path: &str,
    logger: &impl Fn(&str) -> Pin<Box<dyn Future<Output = Result<()>>>>,
) -> Result<()> {
    match vector::generate_embeddings(env, text).await {
        Ok(embeddings) => {
            // Convert to vector metadata
            let vector_metadata = vector::VectorMetadata {
                link_id,
                url,
                bucket_path,
            };

            // Insert vector with metadata
            if let Err(e) = vector::insert_vector(env, vector_metadata, embeddings).await {
                console_error!("Failed to insert vector: {}", e);
            } else {
                logger(&format!("Successfully created embeddings for: {}", url)).await?;
            }
        }
        Err(e) => {
            console_error!("Failed to generate embeddings: {}", e);
        }
    }

    Ok(())
}

/// Search links using vector similarity
pub async fn search_links(env: Env, query: &str) -> Result<String> {
    console_log!("Searching for: {}", query);

    // Query the vector database to get vector IDs
    let vector_ids = match vector::query_vectors(&env, query, 5).await {
        Ok(ids) => ids,
        Err(e) => {
            console_error!("Error querying vectors: {}", e);
            return Ok(format!("⚠️ Error searching: {}", e));
        }
    };

    if vector_ids.is_empty() {
        return Ok("No results found for your query. Try a different search term.".to_string());
    }

    // Build response with matching links
    let mut response = format!("🔍 Search results for '{}'\n\n", query);

    // For each vector ID, get the corresponding link info
    for (i, id) in vector_ids.iter().enumerate() {
        match storage::get_link_by_id(&env, id).await {
            Ok(link_info) => {
                let title_display = link_info.title;

                // Format each result with its details
                response.push_str(&format!(
                    "{}. {} {}\n{}\n\n",
                    i + 1,
                    link_info.type_emoji,
                    title_display,
                    link_info.url,
                ));
            }
            Err(e) => {
                console_error!("Error fetching link info for ID {}: {}", id, e);
                response.push_str(&format!(
                    "{}. ⚠️ Result found but details unavailable (ID: {})\n\n",
                    i + 1,
                    id
                ));
            }
        }
    }

    Ok(response)
}
