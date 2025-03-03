use crate::{d1::DocInfo, models::Update};
use serde_json::json;
use worker::*;

// Telegram API constants
const BOT_TOKEN: &str = "BOT_TOKEN";
const TELEGRAM_API_BASE: &str = "https://api.telegram.org/bot";

fn get_authorized_chat_ids(env: Env) -> Vec<i64> {
    let authorized_chat_ids_str = env.var("AUTHORIZED_CHAT_IDS").unwrap();

    authorized_chat_ids_str
        .to_string()
        .split(',')
        .filter_map(|id_str| id_str.trim().parse::<i64>().ok())
        .collect()
}

fn check_id(env: Env, id: i64) -> bool {
    get_authorized_chat_ids(env).contains(&id)
}

/// Processes an update from Telegram webhook
pub async fn process_update(env: Env, update: Update) -> Result<()> {
    let token = env.secret(BOT_TOKEN)?.to_string();

    let Some(message) = &update.message else {
        return Ok(());
    };

    let chat_id = message.chat.id;
    let Some(text) = &message.text else {
        return Ok(());
    };

    console_log!("Received message: {} from chat_id: {}", text, chat_id);

    if !check_id(env.clone(), chat_id) {
        let message = format!(
            "Sorry, you are not authorized to use this bot. Send this message to bot owner to get access:<pre>{:#?}</pre>",
            update.message.as_ref().unwrap()
        );
        send_message(&token, chat_id, message.as_str()).await?;
        return Ok(());
    }

    // Chat is authorized, process commands
    let response = match text.as_str() {
        "/start" => "Hello! I'm Seen, your knowledge assistant!".to_string(),
        "/help" => "Available commands:
/start - Start the bot
/help - Show this help message
/list - Show link statistics
/search [query] - Search through saved links
/delete [url] - Delete a saved link
Or simply send a URL to save it, or any text to search for it."
            .to_string(),
        "/list" => list_links(env).await,
        _ if text.starts_with("/insert") => {
            let url = &text[7..].trim();
            if url.is_empty() {
                "Please provide a URL to insert, e.g., '/insert https://example.com'".to_string()
            } else {
                insert_link(env, url).await
            }
        }
        _ if text.starts_with("http://") || text.starts_with("https://") => {
            insert_link(env, text).await
        }
        _ if text.starts_with("/search ") => {
            let query = &text[8..];
            if query.trim().is_empty() {
                "Please provide a search query, e.g., '/search cloudflare'".to_string()
            } else {
                search_query(env, query).await
            }
        }
        _ if text.starts_with("/delete ") => {
            let url = &text[8..].trim();
            if url.is_empty() {
                "Please provide a URL to delete, e.g., '/delete https://example.com'".to_string()
            } else {
                delete_link(env, url).await
            }
        }
        _ => search_query(env, text).await,
    };

    // Send the response back to the user
    send_message(&token, chat_id, response.as_str()).await?;

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
        "parse_mode": "HTML",
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
        console_error!(
            "Failed to send message: Status {}, message: {}",
            response.status_code(),
            body.to_string()
        );
        return Err(Error::from("Failed to send message"));
    }

    Ok(())
}

async fn insert_link(env: Env, url: &str) -> String {
    match crate::handlers::insert_link(&env, url).await {
        Ok(link_info) => {
            format!(
                "✅ Document saved!\n\
                {}",
                link_info.format_telegram_message()
            )
        }
        Err(e) => {
            console_error!("Error handling link: {}, error: {}", url, e);
            format!("Error handling link: {}, error: {}", url, e)
        }
    }
}

async fn list_links(env: Env) -> String {
    match crate::d1::get_link_stats(env).await {
        Ok((count, rows)) => {
            let mut ret = format!("Total links saved: <b>{}</b>\n\n", count);
            for (i, row) in rows.iter().enumerate() {
                ret.push_str(&format!(
                    "<b>{}.</b> {} <a href=\"{}\">{}</a>\n\n",
                    i + 1,
                    format_type_emoji(&row.content_type),
                    row.url,
                    row.title
                ));
            }
            ret
        }
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
            for (i, (link_info, _chunk_list)) in response.into_iter().enumerate() {
                ret.push_str(&format!(
                    "<b>{}.</b> {} <a href=\"{}\">{}</a>\n\n",
                    i + 1,
                    crate::telegram::format_type_emoji(&link_info.content_type),
                    link_info.url,
                    link_info.title,
                ));
            }
            ret
        }
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
        }
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
            "{}<a href=\"{}\">{}</a>\n\
            <b>Size:</b> {} ({} chunks)\n\
            <b>Summary:</b>\n{}\n",
            format_type_emoji(&self.content_type),
            self.url,
            self.title,
            crate::utils::format_size(self.size),
            self.chunk_count,
            self.summary
        )
    }
}
