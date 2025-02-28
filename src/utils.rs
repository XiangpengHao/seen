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
