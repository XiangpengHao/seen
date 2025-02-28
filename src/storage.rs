use crate::models::LinkInfoWithURL;
use serde::Deserialize;
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
pub async fn get_link_by_id(env: &Env, id: &str) -> Result<LinkInfoWithURL> {
    let db = env.d1("SEEN_DB")?;

    let query = db
        .prepare("SELECT url, title, content_type, bucket_path, summary FROM links WHERE id = ?")
        .bind(&[id.into()])?
        .first::<LinkRow>(None)
        .await?;

    if let Some(row) = query {
        let type_emoji = format_type_emoji(&row.content_type);

        Ok(LinkInfoWithURL {
            url: row.url,
            title: row.title.unwrap_or("No title available".to_string()),
            content_type: row.content_type,
            type_emoji: type_emoji.to_string(),
            bucket_path: row.bucket_path,
            summary: row.summary.unwrap_or("No summary available".to_string()),
            size: 0,
            created_at: "".to_string(),
        })
    } else {
        Err(Error::from("Link not found"))
    }
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
    id: &str,
    url: &str,
    bucket_path: &str,
    content_type: &str,
    size: usize,
    title: &str,
    summary: &str,
) -> Result<()> {
    let d1 = env.d1("SEEN_DB")?;

    // Insert with bucket path and content type
    let stmt = d1
        .prepare("INSERT INTO links (id, url, created_at, bucket_path, content_type, size, title, summary) VALUES (?, ?, datetime('now'), ?, ?, ?, ?, ?)")
        .bind(&[
            JsValue::from_str(id),
            JsValue::from_str(url),
            JsValue::from_str(bucket_path),
            JsValue::from_str(content_type),
            JsValue::from_f64(size as f64),
            JsValue::from_str(title),
            JsValue::from_str(summary),
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

// Update the struct for D1 row results
#[derive(Deserialize)]
struct LinkRow {
    url: String,
    title: Option<String>,
    content_type: String,
    bucket_path: String,
    summary: Option<String>,
}

/// Find a link by URL in the database
pub async fn find_link_by_url(env: &Env, url: &str) -> Result<LinkInfoWithURL> {
    let db = env.d1("SEEN_DB")?;

    // Query the database
    let query_result = db
        .prepare("SELECT id, url, title, content_type, bucket_path, size, created_at, summary FROM links WHERE url = ? LIMIT 1")
        .bind(&[url.into()])?
        .all()
        .await?;

    let rows = query_result.results::<ExistingLinkRow>()?;

    if let Some(row) = rows.into_iter().next() {
        let type_emoji = format_type_emoji(&row.content_type);

        Ok(LinkInfoWithURL {
            url: row.url,
            title: row.title,
            content_type: row.content_type,
            type_emoji: type_emoji.to_string(),
            bucket_path: row.bucket_path,
            summary: row.summary,
            size: row.size,
            created_at: row.created_at,
        })
    } else {
        Err(Error::from("Link not found"))
    }
}

// Add this struct to handle the query result
#[derive(Deserialize)]
struct ExistingLinkRow {
    url: String,
    title: String,
    content_type: String,
    bucket_path: String,
    summary: String,
    size: usize,
    created_at: String,
}
