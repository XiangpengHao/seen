use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;
use worker::*;

#[allow(unused)]
#[derive(Debug, Serialize, Deserialize)]
pub struct DocInfo {
    pub id: String,
    pub url: String,
    pub created_at: String,
    pub bucket_path: String,
    pub content_type: String,
    pub size: usize,
    pub title: String,
    pub summary: String,
    pub chunk_count: usize,
}

/// Retrieves link statistics from the database
/// Returns the total number of links and the details of the latest 10 links
pub async fn get_link_stats(env: Env) -> Result<(u64, Vec<DocInfo>)> {
    let d1 = env.d1("SEEN_DB")?;

    let count_stmt = d1.prepare("SELECT COUNT(*) FROM links");
    let count_result = count_stmt.run().await?;

    let rows = count_result.results::<serde_json::Value>()?;
    let count = if let Some(row) = rows.first() {
        row.get("COUNT(*)").and_then(|v| v.as_u64()).unwrap_or(0)
    } else {
        0
    };

    let links_stmt = d1.prepare("SELECT * FROM links ORDER BY created_at DESC LIMIT 10");
    let links_result = links_stmt.run().await?;

    let rows = links_result.results::<DocInfo>()?;
    Ok((count, rows))
}

/// Retrieve a link by its ID from the database
pub async fn get_link_by_id(env: &Env, id: &str) -> Result<Option<DocInfo>> {
    let db = env.d1("SEEN_DB")?;

    let query = db
        .prepare("SELECT * FROM links WHERE id = ?")
        .bind(&[id.into()])?
        .first::<DocInfo>(None)
        .await?;

    if let Some(row) = query {
        Ok(Some(row))
    } else {
        Ok(None)
    }
}

/// Save content to R2 bucket
pub async fn save_to_bucket(env: &Env, bucket_path: &str, content: Vec<u8>) -> Result<()> {
    let bucket = env.bucket("SEEN_BUCKET")?;
    bucket.put(bucket_path, content).execute().await?;
    Ok(())
}

pub async fn read_from_bucket(env: &Env, bucket_path: &str) -> Result<Vec<u8>> {
    let bucket = env.bucket("SEEN_BUCKET")?;
    let content = bucket
        .get(bucket_path)
        .execute()
        .await?
        .ok_or(Error::from("Content not found"))?;
    let bytes = content
        .body()
        .ok_or(Error::from("Content not found"))?
        .bytes()
        .await?;
    Ok(bytes.to_vec())
}

/// Save link metadata to database
pub async fn save_link_to_db(env: &Env, row: &DocInfo) -> Result<()> {
    let d1 = env.d1("SEEN_DB")?;

    // Insert with bucket path and content type
    let stmt = d1
        .prepare("INSERT INTO links (id, url, created_at, bucket_path, content_type, size, title, summary, chunk_count) VALUES (?, ?, datetime('now'), ?, ?, ?, ?, ?, ?)")
        .bind(&[
            JsValue::from_str(&row.id),
            JsValue::from_str(&row.url),
            JsValue::from_str(&row.bucket_path),
            JsValue::from_str(&row.content_type),
            JsValue::from_f64(row.size as f64),
            JsValue::from_str(&row.title),
            JsValue::from_str(&row.summary),
            JsValue::from_f64(row.chunk_count as f64),
        ])?;

    // Execute query
    stmt.run().await?;
    Ok(())
}

/// Find a link by URL in the database
pub async fn find_link_by_url(env: &Env, url: &str) -> Result<DocInfo> {
    let db = env.d1("SEEN_DB")?;

    // Query the database
    let query_result = db
        .prepare("SELECT * FROM links WHERE url = ? LIMIT 1")
        .bind(&[url.into()])?
        .all()
        .await?;

    let rows = query_result.results::<DocInfo>()?;

    if let Some(row) = rows.into_iter().next() {
        Ok(row)
    } else {
        Err(Error::from("Link not found"))
    }
}

/// Delete a link from the database by URL
pub async fn delete_link_by_url(env: &Env, url: &str) -> Result<DocInfo> {
    // First, get the link info to return it later and for deletion references
    let link_info = find_link_by_url(env, url).await?;

    // Delete the link from the database
    let db = env.d1("SEEN_DB")?;

    let _delete_result = db
        .prepare("DELETE FROM links WHERE url = ?")
        .bind(&[url.into()])?
        .run()
        .await?;

    console_log!("Deleted link from database, URL: {}", url);

    Ok(link_info)
}

/// Delete content from R2 bucket
pub async fn delete_from_bucket(env: &Env, bucket_path: &str) -> Result<()> {
    let bucket = env.bucket("SEEN_BUCKET")?;
    bucket.delete(bucket_path).await?;
    console_log!("Deleted content from bucket, path: {}", bucket_path);
    Ok(())
}

pub async fn get_all_links(env: &Env) -> Result<Vec<DocInfo>> {
    let d1 = env.d1("SEEN_DB")?;
    let links_stmt = d1.prepare("SELECT * FROM links order by created_at desc");
    let links_result = links_stmt.run().await?;
    let rows = links_result.results::<DocInfo>()?;
    Ok(rows)
}
