use base64::{engine::general_purpose::STANDARD, Engine};
use worker::*;

/// Extracts text from HTML content
pub fn extract_text_from_html(html: &str) -> String {
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

/// Helper function to format file sizes
pub fn format_size(size: usize) -> String {
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

/// Extract title from HTML content
pub fn extract_title_from_html(html: &str) -> Option<String> {
    if let Some(title_match) = html.match_indices("<title>").next() {
        let start_idx = title_match.0 + 7; // "<title>" is 7 chars
        if let Some(end_idx) = html[start_idx..].find("</title>") {
            return Some(html[start_idx..(start_idx + end_idx)].trim().to_string());
        }
    }
    None
}

/// Helper function to determine file extension based on content type
pub fn get_extension_from_content_type(content_type: &str) -> &'static str {
    match content_type.split(';').next().unwrap_or("") {
        "text/html" => "html",
        "application/pdf" => "pdf",
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "application/json" => "json",
        "text/plain" => "txt",
        "text/css" => "css",
        "text/javascript" | "application/javascript" => "js",
        "application/xml" | "text/xml" => "xml",
        _ => "bin", // Default binary extension for unknown types
    }
}

/// Base function to make a request to Gemini API
async fn gemini_api_request(
    env: &Env,
    prompt: &str,
    content: Option<(&str, &[u8])>,
) -> Result<String> {
    let api_key = env.secret("GEMINI_API_KEY")?.to_string();
    let api_url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent?key={}",
        api_key
    );

    // Build parts array for the request
    let mut parts = vec![serde_json::json!({
        "text": prompt
    })];

    // Add binary content if provided
    if let Some((mime_type, data)) = content {
        parts.push(serde_json::json!({
            "inline_data": {
                "mime_type": mime_type,
                "data": STANDARD.encode(data)
            }
        }));
    }

    // Create the request payload
    let payload = serde_json::json!({
        "contents": [{
            "role": "user",
            "parts": parts
        }],
    });

    // Make the request
    let mut headers = Headers::new();
    headers.set("Content-Type", "application/json")?;

    let mut req_init = RequestInit::new();
    req_init
        .with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(wasm_bindgen::JsValue::from_str(&payload.to_string())));

    let request = Request::new_with_init(&api_url, &req_init)?;
    let mut response = Fetch::Request(request).send().await?;

    // Handle errors
    if response.status_code() != 200 {
        let error_text = response.text().await?;
        return Err(Error::from(format!(
            "Gemini API failed: Status {}, Error: {}",
            response.status_code(),
            error_text
        )));
    }

    // Parse the response
    let result = response.json::<serde_json::Value>().await?;

    // Extract the text
    result
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.get(0))
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| Error::from("Failed to parse Gemini API response"))
}

/// Extract text from PDF content using Gemini API
pub async fn extract_text_from_pdf_with_gemini(env: &Env, pdf_content: &[u8]) -> Result<String> {
    gemini_api_request(
        env,
        "Extract the content from this PDF document to markdown format. If there is a figure, extract the text from the figure and describe it in the markdown. The result should be suitable for RAG pipeline. Return only the extracted text, no additional commentary.",
        Some(("application/pdf", pdf_content)),
    ).await
}

/// Generate a summary of content using Gemini API
pub async fn generate_summary_with_gemini(env: &Env, text: &str) -> Result<String> {
    gemini_api_request(
        env,
        &format!("Summarize the following text in exactly 1 dense sentences, as the preview of the content. Be concise and to the point. \n{} ", text),
        None,
    ).await
}

/// Fetch content from a URL
/// Returns the content and the content type
pub async fn fetch_content(link: &str) -> Result<(Vec<u8>, String)> {
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

    Ok((content, content_type))
}
