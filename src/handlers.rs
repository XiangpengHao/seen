use crate::models::{LinkInfo, Update};
use crate::storage;
use crate::utils::{extract_text_from_html, extract_title_from_html, get_extension_from_content_type};
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
    let url_param = url.query_pairs()
        .find(|(key, _)| key == "url")
        .map(|(_, value)| value.to_string());

    // Return error if no URL parameter is provided
    let url_to_save = match url_param {
        Some(u) => u,
        None => return Response::error("Missing 'url' parameter", 400),
    };

    // Validate URL format
    if !url_to_save.starts_with("http://") && !url_to_save.starts_with("https://") {
        return Response::error("Invalid URL format, must start with http:// or https://", 400);
    }

    // Use the handle_link function to process the URL
    match handle_link(env, &url_to_save).await {
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
        },
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
pub async fn handle_link(env: Env, link: &str) -> Result<LinkInfo> {
    // Generate a unique ID for this link
    let link_id = Uuid::new_v4().to_string();
    
    // Set up better request headers
    let mut headers = Headers::new();
    headers.set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")?;
    headers.set("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8")?;
    headers.set("Accept-Language", "en-US,en;q=0.5")?;
    
    // Create request with headers
    let mut req_init = RequestInit::new();
    req_init.with_method(Method::Get).with_headers(headers);
    
    // Make request with proper headers
    let request = Request::new_with_init(link, &req_init)?;
    let mut response = Fetch::Request(request).send().await?;
    
    if response.status_code() != 200 {
        return Err(Error::from(format!("Failed to fetch link: Status {}", response.status_code())));
    }
    
    // Get content type from headers
    let content_type = response
        .headers()
        .get("Content-Type")
        .unwrap_or_else(|_| Some("application/octet-stream".to_string()))
        .unwrap_or_else(|| "application/octet-stream".to_string());
    
    // Determine file extension and emoji based on content type
    let extension = get_extension_from_content_type(&content_type);
    let type_emoji = storage::format_type_emoji(&content_type);
    
    // Generate bucket path with appropriate extension
    let bucket_path = format!("content/{}.{}", link_id, extension);
    
    // Get content as bytes
    let content = response.bytes().await?;
    let content_size = content.len();
    
    // Get current timestamp
    let current_time = js_sys::Date::new_0().to_iso_string().as_string().unwrap();
    
    // Store content in R2 bucket
    storage::save_to_bucket(&env, &bucket_path, content.clone()).await?;
    
    // Extract title and process content for vector storage if HTML
    let mut title: Option<String> = None;
    if content_type.starts_with("text/html") {
        // Convert bytes to string for HTML processing
        if let Ok(html_content) = String::from_utf8(content.to_vec()) {
            // Try to extract title from HTML
            title = extract_title_from_html(&html_content);
            
            // Extract text from HTML
            let extracted_text = extract_text_from_html(&html_content);
            
            // Only create embeddings if we have enough text
            if extracted_text.len() > 10 {
                console_log!("Generating embeddings for HTML content...");
                
                // Generate embeddings
                match vector::generate_embeddings(&env, &extracted_text).await {
                    Ok(embeddings) => {
                        // Store embeddings in Vectorize
                        if let Err(e) = vector::insert_vector(&env, &link_id, embeddings, link, title.clone(), &content_type, &bucket_path).await {
                            console_error!("Failed to insert vector: {}", e);
                        } else {
                            console_log!("Successfully created embeddings for {}", link);
                        }
                    },
                    Err(e) => {
                        console_error!("Failed to generate embeddings: {}", e);
                    }
                }
            } else {
                console_log!("Not enough text content for embeddings");
            }
        }
    }
    
    // Store link info in database
    storage::save_link_to_db(&env, link, &bucket_path, &content_type, content_size, title.as_deref()).await?;
    
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
                    |t| if t.len() > 40 { format!("{}...", &t[0..37]) } else { t.clone() }
                );
                
                // Format each result with its details
                response.push_str(&format!(
                    "{}. {} {} {}\n   {}\n\n",
                    i + 1,
                    link_info.type_emoji,
                    title_display,
                    if link_info.title.is_some() { "" } else { &link_info.url },
                    link_info.url,
                ));
            },
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