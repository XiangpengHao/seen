# Seen 

Personal knowledge management for the impatient.

Read something but forget where it was? Seen remembers what you have seen.

## Features

- Dead simple, just send a link, and search later from whatever you still remember.
- Even more impatient? Install Seen as a browser extension and save any webpage you browse.

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



## Development
### Add a new column to the table:
```SQL
ALTER TABLE links ADD COLUMN chunk_count INTEGER DEFAULT 0;
```


## License

MIT 