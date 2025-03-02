
<p align="center"> <img src="/dev/doc/logo.png" alt="seen_logo" width="450"/> </p>

Read something but forget where it was? Seen remembers what you have seen.

Seen is a simple personal knowledge management tool made for impatient people like me.

## Features

- Dead simple, just send a link, and search later from whatever you still remember.

- A simple telegram bot, no bloatware.

- It cures my allergic to AI by using a lot of AI in this project: document processing, vector search, etc.


## Hosted service
Let me know if you are interested in a hosted version of Seen.


## Self-hosting 

### 1. Create a Telegram Bot

Create a Telegram bot (from [@BotFather](https://t.me/BotFather)) and get an API token.

### 2. Configure Cloudflare Workers

Login to Cloudflare:
```bash
npx wrangler login
```

Clone this repository
```bash
git clone https://github.com/XiangpengHao/seen.git
cd seen
```

#### Setup Telegram 
Add your Telegram bot token to Cloudflare Workers as a secret:
```bash
wrangler secret put BOT_TOKEN
```
When prompted, paste your Telegram bot token.

#### Setup CloudFlare
Add your [CloudFlare account ID and API token](https://developers.cloudflare.com/fundamentals/api/get-started/account-owned-tokens/) to Cloudflare Workers as secrets:
```bash
wrangler secret put CF_ACCOUNT_ID
wrangler secret put CF_API_TOKEN
```

#### Setup Gemini
Seen use Gemini to process documents. You can get a free API key from [Gemini API](https://ai.google.dev/gemini-api/docs/quickstart).

Add your Gemini API key to Cloudflare Workers as a secret:
```bash
wrangler secret put GEMINI_API_KEY
```



### 3. Build and Deploy to Cloudflare Workers

Build and deploy your bot to Cloudflare Workers:

```bash
npx wrangler deploy
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


## License
MIT 