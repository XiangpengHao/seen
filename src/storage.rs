use crate::models::LinkInfoWithURL;
use serde_json::Value;
use wasm_bindgen::JsValue;
use worker::*;

/// Retrieves link statistics from the database
pub async fn get_link_stats(env: Env) -> Result<String> {
    let d1 = env.d1("SEEN_DB")?;

    let count_stmt = d1.prepare("SELECT COUNT(*) FROM links");
    let count_result = count_stmt.run().await?;

    let rows = count_result.results::<serde_json::Value>()?;
    let count = if let Some(row) = rows.first() {
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
            let type_emoji = format_type_emoji(content_type.unwrap_or(""));

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

/// Retrieve a link by its ID from the database
pub async fn get_link_by_id(env: &Env, link_id: &str) -> Result<LinkInfoWithURL> {
    let d1 = env.d1("SEEN_DB")?;

    // Query database to get link info by bucket_path that contains the link_id
    let stmt = d1
        .prepare("SELECT url, created_at, bucket_path, content_type, size, title FROM links WHERE bucket_path LIKE ?")
        .bind(&[JsValue::from_str(&format!("%{}%", link_id))])?;

    let result = stmt.run().await?;
    let rows = result.results::<Value>()?;

    if let Some(row) = rows.first() {
        let url = row
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown URL")
            .to_string();

        let title = row
            .get("title")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let content_type = row
            .get("content_type")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown type")
            .to_string();

        let bucket_path = row
            .get("bucket_path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Format file type emoji based on content type
        let type_emoji = format_type_emoji(content_type.split(';').next().unwrap_or(""));

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

/// Save content to R2 bucket
pub async fn save_to_bucket(env: &Env, bucket_path: &str, content: Vec<u8>) -> Result<()> {
    let bucket = env.bucket("SEEN_BUCKET")?;
    bucket.put(bucket_path, content).execute().await?;
    Ok(())
}

/// Save link metadata to database
pub async fn save_link_to_db(
    env: &Env,
    url: &str,
    bucket_path: &str,
    content_type: &str,
    size: usize,
    title: Option<&str>,
) -> Result<()> {
    let d1 = env.d1("SEEN_DB")?;

    // Insert with bucket path and content type
    let stmt = d1
        .prepare("INSERT INTO links (url, created_at, bucket_path, content_type, size, title) VALUES (?, datetime('now'), ?, ?, ?, ?)")
        .bind(&[
            JsValue::from_str(url),
            JsValue::from_str(bucket_path),
            JsValue::from_str(content_type),
            JsValue::from_f64(size as f64),
            if let Some(t) = title { JsValue::from_str(t) } else { JsValue::null() },
        ])?;

    // Execute query
    stmt.run().await?;
    Ok(())
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
