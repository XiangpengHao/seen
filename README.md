# Telegram Bot on Cloudflare Workers

This project implements a simple Telegram bot running on Cloudflare Workers using Rust and WebAssembly. The bot can respond to basic commands like `/start`, `/help`, and `/echo`.

## Features

- Written in Rust, compiled to WebAssembly, running on Cloudflare Workers
- Handles Telegram webhook events
- Responds to basic commands
- Easily extendable to add more functionality

## Setup Instructions

### 1. Create a Telegram Bot

First, you need to create a Telegram bot and get an API token:

1. Open Telegram and search for `@BotFather`
2. Start a chat with BotFather by clicking on the "Start" button
3. Send the `/newbot` command to create a new bot
4. Follow the prompts to choose a name and username for your bot
5. Once created, BotFather will provide you with a token (keep this token secure!)

### 2. Configure Cloudflare Workers

1. Make sure you have [Wrangler CLI](https://developers.cloudflare.com/workers/wrangler/install-and-update/) installed and configured:
   ```bash
   npm install -g wrangler
   wrangler login
   ```

2. Clone this repository
   ```bash
   git clone <repository-url>
   cd <repository-directory>
   ```

3. Add your Telegram bot token to Cloudflare Workers as a secret:
   ```bash
   wrangler secret put BOT_TOKEN
   ```
   When prompted, paste your Telegram bot token.

### 3. Build and Deploy to Cloudflare Workers

Build and deploy your bot to Cloudflare Workers:

```bash
wrangler publish
```

This will deploy your bot and give you a URL (something like `https://your-bot.your-username.workers.dev`).

### 4. Set up Telegram Webhook

Tell Telegram where to send updates by setting up a webhook. Open a browser and navigate to:

```
https://api.telegram.org/bot<BOT_TOKEN>/setWebhook?url=https://your-bot.your-username.workers.dev/webhook
```

Replace `<BOT_TOKEN>` with your actual bot token and update the Worker URL accordingly.

You should see a response like:
```json
{"ok":true,"result":true,"description":"Webhook was set"}
```

## Bot Commands

The bot responds to the following commands:

- `/start` - Displays a welcome message
- `/help` - Shows a list of available commands
- `/echo <text>` - Echoes back the text you send

## Customizing the Bot

To add more commands or functionality, modify the `process_update` function in `src/lib.rs`. You can extend the match statement to handle additional commands or implement more complex behaviors.

## Troubleshooting

### Common Issues

1. **Deployment Errors**: If you encounter compilation errors, make sure you're using compatible dependencies for WebAssembly. The current implementation uses minimal dependencies to ensure compatibility.

2. **Webhook Setup**: If your bot isn't responding to messages, verify your webhook is set up correctly:
   ```
   https://api.telegram.org/bot<BOT_TOKEN>/getWebhookInfo
   ```

3. **Logs**: Check the Cloudflare Workers logs for any errors:
   ```bash
   wrangler tail
   ```

### Still Having Issues?

- Ensure your Cloudflare Workers account is properly set up
- Double-check that your bot token is correctly stored as a secret
- Verify that your webhook URL is accessible from the internet

## License

MIT 