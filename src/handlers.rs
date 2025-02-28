use std::future::Future;
use std::pin::Pin;

use crate::models::{ContentMetadata, LinkInfo, Update};
use crate::storage;
use crate::utils::{
    extract_text_from_html, extract_text_from_pdf_with_gemini, extract_title_from_html,
    get_extension_from_content_type,
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
    let (content, content_type) = fetch_content(link, &logger).await?;

    // Process metadata and prepare storage
    let (bucket_path, type_emoji, content_size) =
        prepare_storage_metadata(&content_type, &link_id, content.len());
    let current_time = js_sys::Date::new_0().to_iso_string().as_string().unwrap();

    // Save content to bucket
    storage::save_to_bucket(env, &bucket_path, content.clone()).await?;
    logger(&format!("Saved content to bucket: {}", bucket_path)).await?;

    // Process content based on type and generate embeddings
    let title = process_content(
        env,
        &content_type,
        &content,
        &link_id,
        link,
        &bucket_path,
        &logger,
    )
    .await?;

    // Store link info in database
    storage::save_link_to_db(
        env,
        link,
        &bucket_path,
        &content_type,
        content_size,
        title.as_deref(),
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

/// Fetch content from a URL
async fn fetch_content(
    link: &str,
    logger: &impl Fn(&str) -> Pin<Box<dyn Future<Output = Result<()>>>>,
) -> Result<(Vec<u8>, String)> {
    let mut headers = Headers::new();
    headers.set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")?;
    headers.set(
        "Accept",
        "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8",
    )?;
    headers.set("Accept-Language", "en-US,en;q=0.5")?;

    let mut req_init = RequestInit::new();
    req_init.with_method(Method::Get).with_headers(headers);

    let request = Request::new_with_init(link, &req_init)?;
    let mut response = Fetch::Request(request).send().await?;

    if response.status_code() != 200 {
        return Err(Error::from(format!(
            "Failed to fetch link: Status {}",
            response.status_code()
        )));
    }

    let content_type = response
        .headers()
        .get("Content-Type")
        .unwrap_or_else(|_| Some("application/octet-stream".to_string()))
        .unwrap_or_else(|| "application/octet-stream".to_string());

    let content = response.bytes().await?;
    logger(&format!(
        "Downloaded content for {}, size: {}",
        link,
        content.len()
    ))
    .await?;

    Ok((content, content_type))
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
async fn process_content(
    env: &Env,
    content_type: &str,
    content: &[u8],
    link_id: &str,
    link: &str,
    bucket_path: &str,
    logger: &impl Fn(&str) -> Pin<Box<dyn Future<Output = Result<()>>>>,
) -> Result<Option<String>> {
    // Create a mutable metadata struct
    let mut metadata = ContentMetadata {
        link_id,
        url: link,
        content_type,
        bucket_path,
        title: None,
    };

    if content_type.starts_with("text/html") {
        // Process HTML content
        process_html_content(env, content, &mut metadata, logger).await?;
    } else if content_type == "application/pdf" {
        // Process PDF content
        process_pdf_content(env, content, &mut metadata, logger).await?;
    }

    Ok(metadata.title)
}

/// Process HTML content and generate embeddings
async fn process_html_content(
    env: &Env,
    content: &[u8],
    metadata: &mut ContentMetadata<'_>,
    logger: &impl Fn(&str) -> Pin<Box<dyn Future<Output = Result<()>>>>,
) -> Result<()> {
    if let Ok(html_content) = String::from_utf8(content.to_vec()) {
        // Extract title
        metadata.title = extract_title_from_html(&html_content);

        // Extract text
        let extracted_text = extract_text_from_html(&html_content);

        // Generate embeddings if enough text
        if extracted_text.len() > 10 {
            console_log!("Generating embeddings for HTML content...");
            generate_and_store_embeddings(env, &extracted_text, metadata, logger).await?;
        } else {
            console_log!("Not enough text content for embeddings");
        }
    }

    Ok(())
}

/// Process PDF content and generate embeddings
async fn process_pdf_content(
    env: &Env,
    content: &[u8],
    metadata: &mut ContentMetadata<'_>,
    logger: &impl Fn(&str) -> Pin<Box<dyn Future<Output = Result<()>>>>,
) -> Result<()> {
    console_log!("Processing PDF content with Gemini API...");

    // Extract text from PDF
    match extract_text_from_pdf_with_gemini(env, content).await {
        Ok(extracted_text) => {
            // Set title from filename
            metadata.title = Some(
                metadata
                    .url
                    .split('/')
                    .last()
                    .unwrap_or("PDF Document")
                    .replace(".pdf", "")
                    .to_string(),
            );

            // Generate embeddings if enough text
            if extracted_text.len() > 10 {
                console_log!("Generating embeddings for PDF content...");
                generate_and_store_embeddings(env, &extracted_text, metadata, logger).await?;
            } else {
                console_log!("Not enough text content in PDF for embeddings");
            }
        }
        Err(e) => {
            console_error!("Failed to extract text from PDF: {}", e);
        }
    }

    Ok(())
}

/// Generate embeddings and store them in the vector database
async fn generate_and_store_embeddings(
    env: &Env,
    text: &str,
    metadata: &ContentMetadata<'_>,
    logger: &impl Fn(&str) -> Pin<Box<dyn Future<Output = Result<()>>>>,
) -> Result<()> {
    match vector::generate_embeddings(env, text).await {
        Ok(embeddings) => {
            // Convert to vector metadata
            let vector_metadata = vector::VectorMetadata {
                link_id: metadata.link_id,
                url: metadata.url,
                title: metadata.title.clone(),
                content_type: metadata.content_type,
                bucket_path: metadata.bucket_path,
            };

            // Insert vector with metadata
            if let Err(e) = vector::insert_vector(env, vector_metadata, embeddings).await {
                console_error!("Failed to insert vector: {}", e);
            } else {
                logger(&format!(
                    "Successfully created embeddings for: {}",
                    metadata.url
                ))
                .await?;
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
                let title_display = link_info.title.as_ref().map_or_else(
                    || "No title".to_string(),
                    |t| {
                        if t.len() > 40 {
                            format!("{}...", &t[0..37])
                        } else {
                            t.clone()
                        }
                    },
                );

                // Format each result with its details
                response.push_str(&format!(
                    "{}. {} {} {}\n   {}\n\n",
                    i + 1,
                    link_info.type_emoji,
                    title_display,
                    if link_info.title.is_some() {
                        ""
                    } else {
                        &link_info.url
                    },
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
