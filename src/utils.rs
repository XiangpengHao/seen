use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use worker::*;

/// Structured data returned from Gemini API for link processing
#[derive(Debug, Deserialize, Serialize)]
pub struct ProcessedLinkData {
    pub title: String,
    pub summary: String,
    pub chunks: Vec<String>,
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
    inline_content: (&str, &[u8]),
    response_schema: Option<serde_json::Value>,
    previous_response: Option<&str>,
) -> Result<(String, bool)> {
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
    parts.push(serde_json::json!({
        "inline_data": {
            "mime_type": inline_content.0,
            "data": STANDARD.encode(inline_content.1)
        }
    }));

    // Create the request payload with conversation structure if there's a previous response
    let mut payload = if let Some(prev_response) = previous_response {
        serde_json::json!({
            "contents": [
                {
                    "role": "user",
                    "parts": parts
                },
                {
                    "role": "model",
                    "parts": [{"text": prev_response}]
                },
                {
                    "role": "user",
                    "parts": [{"text": "Continue"}]
                }
            ],
        })
    } else {
        serde_json::json!({
            "contents": [{
                "role": "user",
                "parts": parts
            }],
        })
    };

    if let Some(response_schema) = response_schema {
        payload["generationConfig"] = serde_json::json!({
            "responseMimeType": "application/json",
            "responseSchema": response_schema
        });
    }

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

    console_log!("Gemini API response: {}", result);

    // Check if the response was truncated due to token limits
    let was_truncated = result
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("finishReason"))
        .and_then(|f| f.as_str())
        .map(|reason| reason == "MAX_TOKENS")
        .unwrap_or(false);

    // Extract the text
    let text = result
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.get(0))
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| Error::from("Failed to parse Gemini API response"))?;

    Ok((text, was_truncated))
}

/// Process a link with Gemini API and return structured data
pub async fn chunk_and_summary_link(
    env: &Env,
    content: &[u8],
    content_type: &str,
) -> Result<ProcessedLinkData> {
    let initial_prompt =
        "OCR the following content into Markdown without any html tags. Tables should be formatted as markdown tables. \
        Figures should be described in the text, text in the figures should be extracted. \
        Do not surround your output with triple backticks. \
        Chunk the markdown content into sections of roughly 2000 tokens, each chunk should have roughly the same semantic (suitable for embedding). \
        These chunks will be embedded and used in a RAG pipeline. Output in the chunks field, as array.\n\n\
        You should generate a two sentence summary of the document, dense and concise brief, output in the summary field.\n\n\
        You should extract the original title of the document, and if not present, you should generate one based on the content. output in the title field.\n\n";

    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "summary": {
                "type": "string"
            },
            "chunks": {
                "type": "array",
                "items": {
                    "type": "string"
                }
            },
            "title": {
                "type": "string"
            }
        },
        "required": [
            "summary",
            "chunks",
            "title"
        ]
    });

    // Make the initial request
    let (mut response_text, mut was_truncated) = gemini_api_request(
        env,
        initial_prompt,
        (content_type, content),
        Some(schema.clone()),
        None,
    )
    .await?;

    // If the response was truncated, keep sending follow-up requests until we get the complete output
    if was_truncated {
        let mut attempt = 1;
        let max_attempts = 10; // Prevent infinite loops

        while was_truncated && attempt < max_attempts {
            console_log!(
                "Gemini API response was truncated, making follow-up request (attempt {})",
                attempt
            );

            // Make a follow-up request using the conversation approach
            let (continued_text, still_truncated) = gemini_api_request(
                env,
                initial_prompt,
                (content_type, content),
                Some(schema.clone()),
                Some(&response_text),
            )
            .await?;

            // Append the new text to our accumulated response
            // Note: We're just keeping everything as a string until we're done
            response_text.push_str(&continued_text);
            was_truncated = still_truncated;

            attempt += 1;
        }

        if attempt >= max_attempts && was_truncated {
            console_log!("Warning: Reached maximum follow-up attempts, document may be incomplete");
        }
    }

    // Now that we have the complete (or as complete as possible) response, try to parse it
    let data = serde_json::from_str::<ProcessedLinkData>(&response_text).map_err(|e| {
        Error::from(format!(
            "Failed to parse Gemini response into structured data: {}, response: {}",
            e, response_text
        ))
    })?;

    Ok(data)
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
