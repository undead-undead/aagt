//! Notification steps for AAGT pipelines.
//!
//! This module provides ready-to-use pipeline steps for sending notifications
//! via Telegram, Discord, and Email (via webhook/API).

use anyhow::Result;
#[cfg(feature = "trading")]
use crate::trading::pipeline::{Context, Step};
use async_trait::async_trait;
use serde_json::json;
use std::fmt::Debug;

// --- Telegram Notification ---

/// A step that sends a message to a Telegram chat using a bot token.
#[derive(Debug)]
pub struct TelegramStep {
    bot_token: String,
    chat_id: String,
    message_template: String, // Simple template string
}

impl TelegramStep {
    /// Create a new Telegram notification step
    /// 
    /// `message_template` can contain placeholders like `{key}` which will be replaced
    /// by values from `context.data` if they exist and are strings/numbers.
    pub fn new(bot_token: impl Into<String>, chat_id: impl Into<String>, message_template: impl Into<String>) -> Self {
        Self {
            bot_token: bot_token.into(),
            chat_id: chat_id.into(),
            message_template: message_template.into(),
        }
    }

    #[cfg(feature = "trading")]
    fn format_message(&self, ctx: &Context) -> String {
        let mut msg = self.message_template.clone();
        // Simple interpolation: replace {key} with value from ctx.data
        for (key, value) in &ctx.data {
             let placeholder = format!("{{{}}}", key);
             if msg.contains(&placeholder) {
                 if let Some(s) = value.as_str() {
                     msg = msg.replace(&placeholder, s);
                 } else {
                     msg = msg.replace(&placeholder, &value.to_string());
                 }
             }
        }
        // Also replace {input} and {outcome}
        msg = msg.replace("{input}", &ctx.input);
        if let Some(outcome) = &ctx.outcome {
             msg = msg.replace("{outcome}", outcome);
        }
        msg
    }
}

#[cfg(feature = "trading")]
#[async_trait]
impl Step for TelegramStep {
    async fn execute(&self, ctx: &mut Context) -> Result<()> {
        let text = self.format_message(ctx);
        let url = format!("https://api.telegram.org/bot{}/sendMessage", self.bot_token);
        
        let client = reqwest::Client::new();
        let _res = client.post(&url)
            .json(&json!({
                "chat_id": self.chat_id,
                "text": text,
                "parse_mode": "Markdown"
            }))
            .send()
            .await?;
            
        ctx.log(format!("Sent Telegram notification to {}", self.chat_id));
        Ok(())
    }

    fn name(&self) -> &str {
        "TelegramNotification"
    }
}

// --- Discord Notification ---

/// A step that sends a message to a Discord channel via Webhook.
#[derive(Debug)]
pub struct DiscordStep {
    webhook_url: String,
    username: Option<String>,
    avatar_url: Option<String>,
    message_template: String,
}

impl DiscordStep {
    pub fn new(webhook_url: impl Into<String>, message_template: impl Into<String>) -> Self {
        Self {
            webhook_url: webhook_url.into(),
            message_template: message_template.into(),
            username: None,
            avatar_url: None,
        }
    }

    pub fn username(mut self, name: impl Into<String>) -> Self {
        self.username = Some(name.into());
        self
    }
    
    #[cfg(feature = "trading")]
    fn format_message(&self, ctx: &Context) -> String {
        // Reuse logic or abstract it later. For now, duplication is fine for simplicity.
        let mut msg = self.message_template.clone();
        for (key, value) in &ctx.data {
             let placeholder = format!("{{{}}}", key);
             if msg.contains(&placeholder) {
                 if let Some(s) = value.as_str() {
                     msg = msg.replace(&placeholder, s);
                 } else {
                     msg = msg.replace(&placeholder, &value.to_string());
                 }
             }
        }
        msg = msg.replace("{input}", &ctx.input);
        if let Some(outcome) = &ctx.outcome {
             msg = msg.replace("{outcome}", outcome);
        }
        msg
    }
}

#[cfg(feature = "trading")]
#[async_trait]
impl Step for DiscordStep {
    async fn execute(&self, ctx: &mut Context) -> Result<()> {
        let text = self.format_message(ctx);
        let client = reqwest::Client::new();
        
        let mut payload = json!({
            "content": text
        });

        if let Some(u) = &self.username {
            payload["username"] = json!(u);
        }
        if let Some(a) = &self.avatar_url {
            payload["avatar_url"] = json!(a);
        }

        let _res = client.post(&self.webhook_url)
            .json(&payload)
            .send()
            .await?;

        ctx.log("Sent Discord notification");
        Ok(())
    }

    fn name(&self) -> &str {
        "DiscordNotification"
    }
}

// --- Email Notification (Generic Webhook) ---

/// A step that sends an email via a generic HTTP API (like SendGrid/Mailgun).
/// Since SMTP is heavy, we recommend using HTTP APIs for agents.
#[derive(Debug)]
pub struct EmailStep {
    api_url: String,
    api_key: String,
    to: String,
    subject: String,
    provider: EmailProvider,
}

#[derive(Debug)]
pub enum EmailProvider {
    Mailgun { domain: String },
    SendGrid,
    CustomWebhook, // Assumes a generic POST {to, subject, body}
}

impl EmailStep {
    pub fn new_mailgun(api_key: &str, domain: &str, to: &str, subject: &str) -> Self {
        Self {
            api_url: format!("https://api.mailgun.net/v3/{}/messages", domain),
            api_key: api_key.to_string(),
            to: to.to_string(),
            subject: subject.to_string(),
            provider: EmailProvider::Mailgun { domain: domain.to_string() },
        }
    }

    pub fn new_sendgrid(api_key: &str, to: &str, subject: &str) -> Self {
         Self {
            api_url: "https://api.sendgrid.com/v3/mail/send".to_string(),
            api_key: api_key.to_string(),
            to: to.to_string(),
            subject: subject.to_string(),
            provider: EmailProvider::SendGrid,
        }
    }
}

#[cfg(feature = "trading")]
#[async_trait]
impl Step for EmailStep {
    async fn execute(&self, ctx: &mut Context) -> Result<()> {
        let body = format!("Pipeline Report:\n\nInput: {}\nOutcome: {:?}\n\nData: {:?}", 
            ctx.input, ctx.outcome, ctx.data);
            
        let client = reqwest::Client::new();
        
        match &self.provider {
            EmailProvider::Mailgun { .. } => {
                client.post(&self.api_url)
                    .basic_auth("api", Some(&self.api_key))
                    .form(&[
                        ("from", "AAGT Agent <agent@aagt.dev>"),
                        ("to", &self.to),
                        ("subject", &self.subject),
                        ("text", &body)
                    ])
                    .send()
                    .await?;
            },
            EmailProvider::SendGrid => {
                let payload = json!({
                    "personalizations": [{"to": [{"email": self.to}]}],
                    "from": {"email": "agent@aagt.dev"},
                    "subject": self.subject,
                    "content": [{"type": "text/plain", "value": body}]
                });
                
                client.post(&self.api_url)
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .json(&payload)
                    .send()
                    .await?;
            },
            _ => ctx.log("Custom email provider not implemented yet"),
        }

        ctx.log(format!("Sent Email notification to {}", self.to));
        Ok(())
    }

    fn name(&self) -> &str {
        "EmailNotification"
    }
}
