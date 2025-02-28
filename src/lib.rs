use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid;
use wasm_bindgen::JsValue;
use worker::*;

// Environment variable for Telegram bot token
const BOT_TOKEN: &str = "BOT_TOKEN";

// Telegram API base URL
const TELEGRAM_API_BASE: &str = "https://api.telegram.org/bot";

// Cloudflare account ID and API token env vars
const CF_ACCOUNT_ID: &str = "CF_ACCOUNT_ID";
const CF_API_TOKEN: &str = "CF_API_TOKEN";
const VECTORIZE_INDEX_NAME: &str = "seen-index";

// Workers AI API URL
const WORKERS_AI_API_URL: &str = "https://api.cloudflare.com/client/v4/accounts/{account_id}/ai/run/@cf/baai/bge-base-en-v1.5";

// Structures for Workers AI
#[derive(Serialize)]
struct EmbeddingRequest {
    text: Vec<String>,
}

#[derive(Deserialize, Debug)]
struct EmbeddingResponse {
    result: EmbeddingResult,
    success: bool,
}

#[derive(Deserialize, Debug)]
struct EmbeddingResult {
    shape: Vec<usize>,
    data: Vec<Vec<f32>>,
}

// Structures for Vectorize API
#[derive(Serialize)]
struct CreateIndexRequest {
    name: String,
    description: String,
    config: IndexConfig,
}

#[derive(Serialize)]
struct IndexConfig {
    dimensions: usize,
    metric: String,
}

#[derive(Serialize)]
struct VectorInsertRequest {
    id: String,
    values: Vec<f32>,
    metadata: VectorMetadata,
}

#[derive(Serialize, Deserialize)]
struct VectorMetadata {
    url: String,
    title: Option<String>,
    bucket_path: String,
    content_type: String,
}

#[derive(Serialize)]
struct VectorQueryRequest {
    vector: Vec<f32>,
    top_k: usize,
}

#[derive(Deserialize)]
struct VectorQueryResponse {
    result: VectorQueryResult,
    success: bool,
}

#[derive(Deserialize)]
struct VectorQueryResult {
    count: usize,
    matches: Vec<VectorMatch>,
}

#[derive(Deserialize)]
struct VectorMatch {
    id: String,
    score: f32,
    metadata: Option<VectorMetadata>,
}

// Function to generate embeddings using Workers AI
async fn generate_embeddings(env: &Env, text: &str) -> Result<Vec<f32>> {
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
        .with_body(Some(wasm_bindgen::JsValue::from_str(&serde_json::to_string(&embedding_req)?)));
    
    let request = Request::new_with_init(&url, &init)?;
    let mut response = Fetch::Request(request).send().await?;
    
    if response.status_code() != 200 {
        let error_text = response.text().await?;
        console_error!("Failed to generate embeddings: {}", error_text);
        return Err(Error::from("Failed to generate embeddings"));
    }
    
    let embedding_response: EmbeddingResponse = response.json().await?;
    
    if !embedding_response.success || embedding_response.result.data.is_empty() {
        return Err(Error::from("Failed to generate embeddings: empty response"));
    }
    
    Ok(embedding_response.result.data[0].clone())
}

// Function to insert vector into Vectorize index
async fn insert_vector(env: &Env, link_id: &str, values: Vec<f32>, url: &str, title: Option<String>, content_type: &str, bucket_path: &str) -> Result<()> {
    let account_id = env.secret(CF_ACCOUNT_ID)?.to_string();
    let api_token = env.secret(CF_API_TOKEN)?.to_string();
    
    let url_endpoint = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/vectorize/v2/indexes/{}/insert",
        account_id, VECTORIZE_INDEX_NAME
    );
    
    // Create a vector object in JSON format
    let vector_obj = json!({
        "id": link_id.to_string(),
        "values": values,
        "metadata": {
            "url": url.to_string(),
            "bucket_path": bucket_path.to_string(),
            "content_type": content_type.to_string(),
            "title": title
        }
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
    
    console_log!("Vector inserted successfully for link ID: {}", link_id);
    Ok(())
}

// Function to query Vectorize index
async fn query_vectors(env: &Env, query_text: &str, top_k: usize) -> Result<Vec<String>> {
    let account_id = env.secret(CF_ACCOUNT_ID)?.to_string();
    let api_token = env.secret(CF_API_TOKEN)?.to_string();
    
    // Generate embedding for the query text
    let query_vector = generate_embeddings(env, query_text).await?;
    
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/vectorize/v2/indexes/{}/query",
        account_id, VECTORIZE_INDEX_NAME
    );
    
    // Simplify query to just get IDs
    let query_req = VectorQueryRequest {
        vector: query_vector,
        top_k,
    };
    
    let mut headers = Headers::new();
    headers.set("Authorization", &format!("Bearer {}", api_token))?;
    headers.set("Content-Type", "application/json")?;
    
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(wasm_bindgen::JsValue::from_str(&serde_json::to_string(&query_req)?)));
    
    let request = Request::new_with_init(&url, &init)?;
    let mut response = Fetch::Request(request).send().await?;
    
    if response.status_code() != 200 {
        let error_text = response.text().await?;
        console_error!("Failed to query vectors: {}", error_text);
        return Err(Error::from("Failed to query vectors"));
    }
    
    let query_response: VectorQueryResponse = response.json().await?;
    
    if !query_response.success {
        return Err(Error::from("Failed to query vectors: unsuccessful response"));
    }
    
    // Just return the vector IDs
    Ok(query_response.result.matches.iter().map(|m| m.id.clone()).collect())
}

// Function to extract text from HTML
fn extract_text_from_html(html: &str) -> String {
    // Simple HTML text extraction: remove tags and excessive whitespace
    // This is a basic implementation; for production, consider a more robust HTML parser
    let no_tags = html.replace("<[^>]*>", " ");
    let no_extra_spaces = no_tags.replace("\\s+", " ");
    
    // Limit text length to prevent issues with too large vectors
    // Workers AI might have limits on input size
    if no_extra_spaces.len() > 32000 {
        no_extra_spaces[0..32000].to_string()
    } else {
        no_extra_spaces
    }
}

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    console_error_panic_hook::set_once();

    // Get request URL
    let url = req.url()?;
    let path = url.path();

    match path {
        "/" => Response::ok("Telegram Bot is running!"),
        "/webhook" => handle_webhook(req, env).await,
        "/link" => handle_link_request(req, env).await,
        _ => Response::error("Not Found", 404),
    }
}

async fn handle_webhook(mut req: Request, env: Env) -> Result<Response> {
    let update = req.json::<Update>().await?;
    process_update(env, update).await?;
    Response::ok("OK")
}

async fn process_update(env: Env, update: Update) -> Result<()> {
    let token = env.secret(BOT_TOKEN)?.to_string();
    // Check if the update contains a message with text
    if let Some(message) = update.message {
        if let Some(text) = message.text {
            console_log!("Received message: {}", text);

            // Generate a response based on the command
            let response = match text.as_str() {
                "/start" => "Hello! I'm your Telegram bot running on Cloudflare Workers with Rust!".to_string(),
                "/help" => "Available commands:\n/start - Start the bot\n/help - Show this help message\n/echo <text> - Echo back your text\n/list - Show link statistics\n/search <query> - Search through saved links".to_string(),
                "/list" => get_link_stats(env).await?,
                _ if text.starts_with("/search ") => {
                    // Extract the search query
                    let query = &text[8..];
                    if query.trim().is_empty() {
                        "Please provide a search query, e.g., '/search cloudflare'".to_string()
                    } else {
                        search_links(env, query).await?
                    }
                },
                _ if text.starts_with("/echo ") => text[6..].to_string(),
                _ if text.starts_with("http://") || text.starts_with("https://") => {
                    // Get detailed information from handle_link
                    let link_info = handle_link(env, &text).await?;
                    
                    format!(
                        "✅ Link saved successfully!\n\n\
                        URL: {}\n\
                        Type: {} {}\n\
                        Size: {}\n\
                        Saved: {}\n\
                        Bucket Path: {}\n\n\
                        Use /list to see all saved links.\n\
                        Use /search <query> to search through saved content.",
                        text,
                        link_info.type_emoji,
                        link_info.content_type,
                        format_size(link_info.size),
                        link_info.timestamp,
                        link_info.bucket_path
                    )
                },
                _ => "I don't understand that command. Try /help for a list of available commands.".to_string()
            };

            // Send the response back to the user
            send_message(&token, message.chat.id, response.as_str()).await?;
        }
    }

    Ok(())
}

// Create a structure to return information about the saved link
#[derive(Debug)]
struct LinkInfo {
    content_type: String,
    type_emoji: String,
    size: usize,
    timestamp: String,
    bucket_path: String,
}

// New function to retrieve link info by ID
async fn get_link_by_id(env: &Env, link_id: &str) -> Result<LinkInfoWithURL> {
    let d1 = env.d1("SEEN_DB")?;
    
    // Query database to get link info by bucket_path that contains the link_id
    let stmt = d1
        .prepare("SELECT url, created_at, bucket_path, content_type, size, title FROM links WHERE bucket_path LIKE ?")
        .bind(&[JsValue::from_str(&format!("%{}%", link_id))])?;
    
    let result = stmt.run().await?;
    let rows = result.results::<serde_json::Value>()?;
    
    if let Some(row) = rows.get(0) {
        let url = row.get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown URL")
            .to_string();
        
        let title = row.get("title")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        
        let content_type = row.get("content_type")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown type")
            .to_string();
        
        let bucket_path = row.get("bucket_path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        
        // Format file type emoji based on content type
        let type_emoji = match content_type.split(';').next().unwrap_or("") {
            "text/html" => "🌐",
            "application/pdf" => "📄",
            t if t.starts_with("image/") => "🖼️",
            "text/plain" => "📝",
            _ => "📁",
        };
        
        return Ok(LinkInfoWithURL {
            url,
            title,
            content_type,
            type_emoji: type_emoji.to_string(),
            bucket_path,
        });
    }
    
    Err(Error::from(format!("Link with ID {} not found", link_id)))
}

// Define a structure to hold link info with URL
#[derive(Debug)]
struct LinkInfoWithURL {
    url: String,
    title: Option<String>,
    content_type: String,
    type_emoji: String,
    bucket_path: String,
}

// Update the search_links function to use the new approach
async fn search_links(env: Env, query: &str) -> Result<String> {
    console_log!("Searching for: {}", query);
    
    // Query the vector database to get vector IDs
    let vector_ids = match query_vectors(&env, query, 5).await {
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
        match get_link_by_id(&env, id).await {
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

async fn handle_link(env: Env, link: &str) -> Result<LinkInfo> {
    // Generate a unique ID for this link
    let link_id = uuid::Uuid::new_v4().to_string();
    
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
    let (extension, type_emoji) = match content_type.as_str().split(';').next().unwrap_or("") {
        "text/html" => ("html", "🌐"),
        "application/pdf" => ("pdf", "📄"),
        "image/jpeg" => ("jpg", "🖼️"),
        "image/png" => ("png", "🖼️"),
        "image/gif" => ("gif", "🖼️"),
        "application/json" => ("json", "📋"),
        "text/plain" => ("txt", "📝"),
        "text/css" => ("css", "🎨"),
        "text/javascript" | "application/javascript" => ("js", "📜"),
        "application/xml" | "text/xml" => ("xml", "📰"),
        _ => ("bin", "📁")  // Default binary extension for unknown types
    };
    
    // Generate bucket path with appropriate extension
    let bucket_path = format!("content/{}.{}", link_id, extension);
    
    // Get content as bytes
    let content = response.bytes().await?;
    let content_size = content.len();
    
    // Get current timestamp
    let current_time = js_sys::Date::new_0().to_iso_string().as_string().unwrap();
    
    // Get R2 bucket and store content
    let bucket = env.bucket("SEEN_BUCKET")?;
    bucket.put(&bucket_path, content.clone()).execute().await?;
    
    // For HTML content, extract text and create embeddings
    let mut title: Option<String> = None;
    if content_type.starts_with("text/html") {
        // Convert bytes to string for HTML processing
        if let Ok(html_content) = String::from_utf8(content.to_vec()) {
            // Try to extract title from HTML
            if let Some(title_match) = html_content.match_indices("<title>").next() {
                let start_idx = title_match.0 + 7; // "<title>" is 7 chars
                if let Some(end_idx) = html_content[start_idx..].find("</title>") {
                    title = Some(html_content[start_idx..(start_idx + end_idx)].trim().to_string());
                }
            }
            
            // Extract text from HTML
            let extracted_text = extract_text_from_html(&html_content);
            
            // Only create embeddings if we have enough text
            if extracted_text.len() > 10 {
                console_log!("Generating embeddings for HTML content...");
                
                // Generate embeddings
                match generate_embeddings(&env, &extracted_text).await {
                    Ok(embeddings) => {
                        // Store embeddings in Vectorize
                        if let Err(e) = insert_vector(&env, &link_id, embeddings, link, title.clone(), &content_type, &bucket_path).await {
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
    
    // Store link info in D1 database
    let d1 = env.d1("SEEN_DB")?;
    
    // Insert with bucket path and content type
    let stmt = d1
        .prepare("INSERT INTO links (url, created_at, bucket_path, content_type, size, title) VALUES (?, datetime('now'), ?, ?, ?, ?)")
        .bind(&[
            JsValue::from_str(link),
            JsValue::from_str(&bucket_path),
            JsValue::from_str(&content_type),
            JsValue::from_f64(content_size as f64),
            if let Some(t) = &title { JsValue::from_str(t) } else { JsValue::null() },
        ])?;
    
    // Execute query
    stmt.run().await?;
    
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

// Helper function to format file sizes
fn format_size(size: usize) -> String {
    if size < 1024 {
        format!("{} bytes", size)
    } else if size < 1024 * 1024 {
        format!("{:.1} KB", size as f64 / 1024.0)
    } else if size < 1024 * 1024 * 1024 {
        format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", size as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

async fn get_link_stats(env: Env) -> Result<String> {
    let d1 = env.d1("SEEN_DB")?;

    let count_stmt = d1.prepare("SELECT COUNT(*) FROM links");
    let count_result = count_stmt.run().await?;

    let rows = count_result.results::<serde_json::Value>()?;
    let count = if let Some(row) = rows.get(0) {
        row.get("COUNT(*)").and_then(|v| v.as_u64()).unwrap_or(0)
    } else {
        0
    };

    let links_stmt =
        d1.prepare("SELECT url, created_at, bucket_path, content_type FROM links ORDER BY created_at DESC LIMIT 10");
    let links_result = links_stmt.run().await?;

    let rows = links_result.results::<serde_json::Value>()?;
    let mut links = Vec::new();

    for row in rows {
        if let (Some(url), Some(timestamp), bucket_path, content_type) = (
            row.get("url").and_then(|v| v.as_str()),
            row.get("created_at").and_then(|v| v.as_str()),
            row.get("bucket_path").and_then(|v| v.as_str()),
            row.get("content_type").and_then(|v| v.as_str()),
        ) {
            let status = if bucket_path.is_some() { "✅" } else { "⏳" };

            // Format file type emoji based on content type
            let type_emoji = match content_type.unwrap_or("").split(';').next().unwrap_or("") {
                "text/html" => "🌐",
                "application/pdf" => "📄",
                t if t.starts_with("image/") => "🖼️",
                "text/plain" => "📝",
                _ => "📁",
            };

            links.push((
                url.to_string(),
                timestamp.to_string(),
                status.to_string(),
                type_emoji.to_string(),
            ));
        }
    }

    // Build response
    let mut response = format!("Total links saved: {}\n\n", count);

    if !links.is_empty() {
        response.push_str("Recent links:\n");

        for (i, (link, timestamp, status, type_emoji)) in links.iter().enumerate() {
            response.push_str(&format!(
                "{}. {} {} {} ({})\n",
                i + 1,
                status,
                type_emoji,
                link,
                timestamp
            ));
        }
    } else {
        response.push_str("No links saved yet.");
    }

    Ok(response)
}

async fn send_message(token: &str, chat_id: i64, text: &str) -> Result<()> {
    // Create the API URL for sending messages
    let url = format!("{}{}/sendMessage", TELEGRAM_API_BASE, token);

    // Create request JSON
    let body = json!({
        "chat_id": chat_id,
        "text": text
    });

    // Send the request
    let mut headers = Headers::new();
    headers.set("Content-Type", "application/json")?;

    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(wasm_bindgen::JsValue::from_str(&body.to_string())));

    let request = Request::new_with_init(&url, &init)?;
    let response = Fetch::Request(request).send().await?;

    // Check status code
    if response.status_code() != 200 {
        console_error!("Failed to send message: Status {}", response.status_code());
        return Err(Error::from("Failed to send message"));
    }

    Ok(())
}

// Telegram Update structure
#[derive(Deserialize, Serialize)]
struct Update {
    update_id: i64,
    #[serde(default)]
    message: Option<Message>,
}

// Telegram Message structure
#[derive(Deserialize, Serialize)]
struct Message {
    message_id: i64,
    chat: Chat,
    #[serde(default)]
    text: Option<String>,
}

// Telegram Chat structure
#[derive(Deserialize, Serialize)]
struct Chat {
    id: i64,
}

async fn handle_link_request(req: Request, env: Env) -> Result<Response> {
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

    // Use the existing handle_link function to process the URL
    match handle_link(env, &url_to_save).await {
        Ok(link_info) => {
            // Create JSON response with the result
            let response_data = json!({
                "success": true,
                "url": url_to_save,
                "content_type": link_info.content_type,
                "size": format_size(link_info.size),
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
