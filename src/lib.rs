use worker::*;
mod d1;
mod handlers;
mod models;
mod telegram;
mod utils;
mod vector;

// Use the console_error_panic_hook for panic handling
#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    console_error_panic_hook::set_once();

    // Get request URL
    let url = req.url()?;
    let path = url.path();

    match path {
        "/" => Response::ok("Telegram Bot is running!"),
        "/webhook" => handlers::handle_webhook(req, env).await,
        _ => Response::error("Not Found", 404),
    }
}
