use worker::*;
use serde::{Deserialize, Serialize};
use serde_json::json;

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
    // Get bot token from environment
    let token = match env.secret(BOT_TOKEN) {
        Ok(token) => token.to_string(),
        Err(e) => {
            console_error!("Failed to get bot token: {:?}", e);
            return Response::error("Internal Server Error", 500);
        }
    };
    
    // Parse the update from request body
    let update = match req.json::<Update>().await {
        Ok(update) => update,
        Err(e) => {
            console_error!("Failed to parse update: {:?}", e);
            return Response::error("Bad Request", 400);
        }
    };
    
    // Process the update
    if let Err(e) = process_update(&token, update).await {
        console_error!("Failed to process update: {:?}", e);
        return Response::error("Internal Server Error", 500);
    }
    
    Response::ok("OK")
}

async fn process_update(token: &str, update: Update) -> Result<()> {
    // Check if the update contains a message with text
    if let Some(message) = update.message {
        if let Some(text) = message.text {
            console_log!("Received message: {}", text);
            
            // Generate a response based on the command
            let response = match text.as_str() {
                "/start" => "Hello! I'm your Telegram bot running on Cloudflare Workers with Rust!",
                "/help" => "Available commands:\n/start - Start the bot\n/help - Show this help message\n/echo <text> - Echo back your text",
                _ if text.starts_with("/echo ") => &text[6..],
                _ => "I don't understand that command. Try /help for a list of available commands."
            };
            
            // Send the response back to the user
            send_message(token, message.chat.id, response).await?;
        }
    }
    
    Ok(())
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
#[derive(Deserialize)]
struct Update {
    update_id: i64,
    #[serde(default)]
    message: Option<Message>,
}

// Telegram Message structure
#[derive(Deserialize)]
struct Message {
    message_id: i64,
    chat: Chat,
    #[serde(default)]
    text: Option<String>,
}

// Telegram Chat structure
#[derive(Deserialize)]
struct Chat {
    id: i64,
}
