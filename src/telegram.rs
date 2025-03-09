use crate::{
    d1::{read_from_bucket, save_to_bucket, DocInfo},
    models::Update,
    vector,
};
use serde_json::json;
use vector_lite::ANNIndexOwned;
use wasm_bindgen::JsValue;
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
        "/help" => html_escape::encode_text(
            "Available commands:
/start - Start the bot
/help - Show this help message
/list - Show link statistics
/search <query> - Search through saved links
/delete <url> - Delete a saved link
/delete_vector <id> - Delete a vector by id
/upgrade - Upgrade vector index
Or simply send a URL to save it, or any text to search for it.",
        )
        .to_string(),
        "/list" => list_links(env).await,
        "/upgrade" => match upgrade_vector_index(env).await {
            Ok((total_ids, migrated)) => format!(
                "Vector index upgraded. Total IDs: {}, Migrated: {}",
                total_ids, migrated
            ),
            Err(e) => format!("Error upgrading vector index: {}", e),
        },
        _ if text.starts_with("/delete_vector ") => {
            let id = &text[15..].trim();
            if id.is_empty() {
                "Please provide a vector id to delete, e.g., '/delete_vector 123'".to_string()
            } else {
                delete_vector(env, id).await
            }
        }
        _ if text.starts_with("/insert ") => {
            let url = &text[8..].trim();
            if url.is_empty() {
                "Please provide a URL to insert, e.g., '/insert https://example.com'".to_string()
            } else {
                insert_link(env, url).await
            }
        }
        _ if text.starts_with("http://") || text.starts_with("https://") => {
            insert_link(env, text).await
        }
        _ if text.starts_with("/search cf ") => {
            let query = &text[11..];
            if query.trim().is_empty() {
                "Please provide a search query, e.g., '/search cf cloudflare'".to_string()
            } else {
                search_query(env, query, true).await
            }
        }
        _ if text.starts_with("/search ") => {
            let query = &text[8..];
            if query.trim().is_empty() {
                "Please provide a search query, e.g., '/search cloudflare'".to_string()
            } else {
                search_query(env, query, false).await
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
        _ => search_query(env, text, false).await,
    };

    // Send the response back to the user
    send_message(&token, chat_id, response.as_str()).await?;

    Ok(())
}

pub async fn delete_vector(env: Env, id: &str) -> String {
    vector::delete_vectors_by_prefix(&env, id, 10)
        .await
        .unwrap();
    let mut vector_lite = vector::get_vector_lite(&env).await.unwrap();
    for i in 0..10 {
        vector_lite.delete_by_id(&format!("{}-{}", id, i));
    }
    vector::save_vector_lite(&env, &vector_lite).await.unwrap();
    "Vector deleted".to_string()
}

pub async fn upgrade_vector_index(env: Env) -> Result<(usize, usize)> {
    let mut index = match read_from_bucket(&env, "vector_lite.bin").await {
        Ok(existing) => vector_lite::VectorLite::<768>::from_bytes(&existing),
        Err(_e) => vector_lite::VectorLite::<768>::new(4, 20),
    };

    // Create embedding table if it doesn't exist
    let db = env.d1("SEEN_DB")?;
    let create_table_stmt = db.prepare(
        "
        CREATE TABLE IF NOT EXISTS embeddings (
            vector_id TEXT PRIMARY KEY,
            vector BLOB NOT NULL,
            link_id TEXT NOT NULL,
            FOREIGN KEY (link_id) REFERENCES links(id)
        )
    ",
    );
    create_table_stmt.run().await?;

    let links = crate::d1::get_all_links(&env).await?;

    let mut ids = vec![];
    for link in links {
        let id = link.id.as_str();
        for chunk in 0..link.chunk_count {
            let vector_id = format!("{}-{}", id, chunk);
            ids.push(vector_id);
        }
    }

    let total_ids = ids.len();
    let mut migrated = index.len();

    let new_ids = ids.iter().skip(index.len()).collect::<Vec<_>>();
    // Get vectors in batches of 20
    for chunk in new_ids.chunks(20).take(15) {
        let chunk_as_str: Vec<&str> = chunk.iter().map(|s| s.as_str()).collect();
        let chunk_vectors = vector::get_vector_by_id(&env, &chunk_as_str).await?;
        migrated += chunk.len();

        for (id, vector) in chunk.into_iter().zip(chunk_vectors) {
            // Insert into database
            // Parse the ID to get link_id and chunk_index
            let parts: Vec<&str> = id.split('-').collect();
            let link_id = parts[0..parts.len() - 1].join("-");

            // Insert or replace the vector in the database
            let stmt = db
                .prepare(
                    "
                    INSERT OR REPLACE INTO embeddings (vector_id, vector, link_id)
                    VALUES (?, ?, ?)
                ",
                )
                .bind(&[
                    (*id).into(),
                    JsValue::from(js_sys::Float32Array::from(vector.as_slice().as_ref())),
                    link_id.into(),
                ])?;

            stmt.run().await?;

            index.insert(vector, id.to_string());
        }
    }

    let bytes = index.to_bytes();
    save_to_bucket(&env, "vector_lite.bin", bytes).await?;

    Ok((total_ids, migrated))
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
                    html_escape::encode_text(&row.title),
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

async fn search_query(env: Env, query: &str, search_from_cf: bool) -> String {
    let result = crate::handlers::search_links(env, query, search_from_cf).await;
    match result {
        Ok(response) => {
            let mut ret = format!("🔍 Search results for '{}'\n\n", query);
            for (i, (link_info, score)) in response.into_iter().enumerate() {
                ret.push_str(&format!(
                    "<b>{}.</b> {} <a href=\"{}\">{}</a> ({:.2})\n\n",
                    i + 1,
                    crate::telegram::format_type_emoji(&link_info.content_type),
                    link_info.url,
                    html_escape::encode_text(&link_info.title),
                    score,
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
                html_escape::encode_text(&link_info.title),
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
            html_escape::encode_text(&self.title),
            crate::utils::format_size(self.size),
            self.chunk_count,
            html_escape::encode_text(&self.summary)
        )
    }
}
