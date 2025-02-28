use crate::models::Update;
use serde_json::json;
use worker::*;

// Telegram API constants
const BOT_TOKEN: &str = "BOT_TOKEN";
const TELEGRAM_API_BASE: &str = "https://api.telegram.org/bot";

/// Processes an update from Telegram webhook
pub async fn process_update(env: Env, update: Update) -> Result<()> {
    let token = env.secret(BOT_TOKEN)?.to_string();
    
    // Check if the update contains a message with text
    if let Some(message) = update.message {
        if let Some(text) = message.text {
            console_log!("Received message: {}", text);

            // Generate a response based on the command
            let response = match text.as_str() {
                "/start" => "Hello! I'm your Telegram bot running on Cloudflare Workers with Rust!".to_string(),
                "/help" => "Available commands:\n/start - Start the bot\n/help - Show this help message\n/echo <text> - Echo back your text\n/list - Show link statistics\n/search <query> - Search through saved links".to_string(),
                "/list" => crate::storage::get_link_stats(env).await?,
                _ if text.starts_with("/search ") => {
                    // Extract the search query
                    let query = &text[8..];
                    if query.trim().is_empty() {
                        "Please provide a search query, e.g., '/search cloudflare'".to_string()
                    } else {
                        crate::handlers::search_links(env, query).await?
                    }
                },
                _ if text.starts_with("/echo ") => text[6..].to_string(),
                _ if text.starts_with("http://") || text.starts_with("https://") => {
                    // Get detailed information from handle_link
                    let link_info = crate::handlers::handle_link(env, &text).await?;
                    
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
                        crate::utils::format_size(link_info.size),
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

/// Sends a message to a Telegram chat
pub async fn send_message(token: &str, chat_id: i64, text: &str) -> Result<()> {
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
