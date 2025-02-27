use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid;
use wasm_bindgen::JsValue;
use worker::*;

// Environment variable for Telegram bot token
const BOT_TOKEN: &str = "BOT_TOKEN";

// Telegram API base URL
const TELEGRAM_API_BASE: &str = "https://api.telegram.org/bot";

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    console_error_panic_hook::set_once();

    // Get request URL
    let url = req.url()?;
    let path = url.path();

    match path {
        "/" => Response::ok("Telegram Bot is running!"),
        "/webhook" => handle_webhook(req, env).await,
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
                "/help" => "Available commands:\n/start - Start the bot\n/help - Show this help message\n/echo <text> - Echo back your text\n/list - Show link statistics".to_string(),
                "/list" => get_link_stats(env).await?,
                _ if text.starts_with("/echo ") => text[6..].to_string(),
                _ if text.starts_with("http://") || text.starts_with("https://") => {
                    handle_link(env, &text).await?;
                    "I received your link! Here's what you sent me: ".to_string() + &text
                },
                _ => "I don't understand that command. Try /help for a list of available commands.".to_string()
            };

            // Send the response back to the user
            send_message(&token, message.chat.id, response.as_str()).await?;
        }
    }

    Ok(())
}

async fn handle_link(env: Env, link: &str) -> Result<()> {
    let link_id = uuid::Uuid::new_v4().to_string();

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

    let extension = match content_type.as_str().split(';').next().unwrap_or("") {
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
        _ => "bin",
    };

    let bucket_path = format!("content/{}.{}", link_id, extension);

    let content = response.bytes().await?;

    // Get R2 bucket and store original content
    let bucket = env.bucket("SEEN_BUCKET")?;
    bucket.put(&bucket_path, content.clone()).execute().await?;

    // Store link info in D1 database
    let d1 = env.d1("SEEN_DB")?;

    // Insert with bucket path and content type
    let stmt = d1
        .prepare("INSERT INTO links (url, created_at, bucket_path, content_type) VALUES (?, datetime('now'), ?, ?)")
        .bind(&[
            JsValue::from_str(link),
            JsValue::from_str(&bucket_path),
            JsValue::from_str(&content_type),
        ])?;

    // Execute query
    stmt.run().await?;

    Ok(())
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
