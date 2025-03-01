use crate::{d1::DocInfo, models::Update};
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
                "/start" => "Hello! I'm Seen, your knowledge assistant!".to_string(),
                "/help" => "Available commands:\n/start - Start the bot\n/help - Show this help message\n/list - Show link statistics\n/search <query> - Search through saved links\n/delete <url> - Delete a saved link\nOr simply send a URL to save it, or any text to search for it.".to_string(),
                "/list" => list_links(env).await,
                _ if text.starts_with("http://") || text.starts_with("https://") => {
                    // Get detailed information from handle_link
                    match crate::handlers::insert_link(&env, &text).await {
                        Ok(link_info) => {
                            format!(
                                "✅ Document saved!\n\
                                {}",
                                link_info.format_telegram_message()
                            )
                        }
                        Err(e) => {
                            console_error!("Error handling link: {}", e);
                            format!("Error handling link: {}", e)
                        }
                    }
                   
                },
                _ if text.starts_with("/search ") => {
                    // Extract the search query
                    let query = &text[8..];
                    if query.trim().is_empty() {
                        "Please provide a search query, e.g., '/search cloudflare'".to_string()
                    } else {
                        search_query(env, query).await
                    }
                },
                _ if text.starts_with("/delete ") => {
                    // Extract the URL to delete
                    let url = &text[8..].trim();
                    if url.is_empty() {
                        "Please provide a URL to delete, e.g., '/delete https://example.com'".to_string()
                    } else {
                        delete_link(env, url).await
                    }
                },
                _ => {
                    search_query(env, &text).await
                }
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
        "text": text,
        "parse_mode": "HTML"
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
        console_error!("Failed to send message: Status {}, message: {}", response.status_code(), body.to_string());
        return Err(Error::from("Failed to send message"));
    }

    Ok(())
}

async fn list_links(env: Env) -> String {
    match crate::d1::get_link_stats(env).await {
        Ok((count, rows)) => {
            let mut ret = format!("Total links saved: {}\n\n", count);
            for row in rows {
                ret.push_str(&row.format_telegram_message());
            }
            ret
        },
        Err(e) => {
            console_error!("Error listing links: {}", e);
            format!("Error listing links: {}", e)
        }
    }
}
async fn search_query(env: Env, query: &str) -> String {
    let result = crate::handlers::search_links(env, query).await;
    match result {
        Ok(response) => {
            let mut ret = format!("🔍 Search results for '{}'\n\n", query);
            for (i, (link_info, chunk_list)) in response.into_iter().enumerate() {
                ret.push_str(&format!(
                    "{}. {} <b>{}</b> \n(chunks: {})\n{}\n{}\n\n",
                    i + 1,
                    crate::telegram::format_type_emoji(&link_info.content_type),
                    link_info.title,
                    chunk_list.iter().map(|chunk_id| chunk_id.to_string()).collect::<Vec<String>>().join(", "),
                    link_info.url,
                    link_info.summary
                ));
            }
            ret
        },
        Err(e) => {
            console_error!("Error searching links: {}", e);
            format!("Error searching links: {}", e)
        }
    }
}

async fn delete_link(env: Env, url: &str) -> String {
    match crate::handlers::delete_link(&env, url).await {
        Ok(link_info) => {
            format!(
                "✅ Successfully deleted:\n\
                <b>URL:</b> {}\n\
                <b>Title:</b> {}\n\
                <b>Type:</b> {} {}\n",
                link_info.url,
                link_info.title,
                format_type_emoji(&link_info.content_type),
                link_info.content_type
            )
        },
        Err(e) => {
            console_error!("Error deleting link: {}", e);
            format!("Error deleting link: {}", e)
        }
    }
}

/// Helper function to determine file type emoji based on content type
pub fn format_type_emoji(content_type: &str) -> &'static str {
    match content_type.split(';').next().unwrap_or("") {
        "text/html" => "🌐",
        "application/pdf" => "📄",
        t if t.starts_with("image/") => "🖼️",
        "text/plain" => "📝",
        _ => "📁",
    }
}

impl DocInfo {
    fn format_telegram_message(&self) -> String {
        format!(
            "<b>URL:</b> {}\n\
            <b>Title:</b> {}\n\
            <b>Type:</b> {} {}\n\
            <b>Size:</b> {}\n\
            <b>Saved:</b> {}\n\
            <b>Chunks:</b> {}\n\
            <b>Summary:</b>\n{}\n",
            self.url,
            self.title,
            format_type_emoji(&self.content_type),
            self.content_type,
            crate::utils::format_size(self.size),
            self.created_at,
            self.chunk_count,
            self.summary
        )
    }
}
