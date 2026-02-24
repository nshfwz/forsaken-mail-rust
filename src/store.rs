use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use regex::Regex;
use serde::Serialize;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub id: String,
    pub mailbox: String,
    pub to: String,
    pub from: String,
    pub subject: String,
    pub date: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, Vec<String>>,
    pub received_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageSummary {
    pub id: String,
    pub from: String,
    pub subject: String,
    pub date: DateTime<Utc>,
    pub has_html: bool,
    pub preview: String,
    pub received_at: DateTime<Utc>,
}

#[derive(Default)]
struct StoreInner {
    by_mailbox: HashMap<String, Vec<Message>>,
}

#[derive(Clone)]
pub struct Store {
    inner: Arc<RwLock<StoreInner>>,
    max_messages: usize,
    ttl: Duration,
}

impl Store {
    pub fn new(max_messages: usize, ttl_minutes: i64) -> Self {
        Self {
            inner: Arc::new(RwLock::new(StoreInner::default())),
            max_messages,
            ttl: Duration::minutes(ttl_minutes.max(1)),
        }
    }

    pub async fn add(&self, mailbox: &str, mut message: Message) {
        let now = Utc::now();
        let mailbox = mailbox.trim().to_ascii_lowercase();

        if message.received_at.timestamp() == 0 {
            message.received_at = now;
        }
        if message.date.timestamp() == 0 {
            message.date = message.received_at;
        }
        if message.mailbox.is_empty() {
            message.mailbox = mailbox.clone();
        }

        let mut inner = self.inner.write().await;
        inner
            .by_mailbox
            .entry(mailbox.clone())
            .or_default()
            .push(message);
        prune_mailbox(
            &mut inner.by_mailbox,
            &mailbox,
            now,
            self.ttl,
            self.max_messages,
        );
    }

    pub async fn list(&self, mailbox: &str) -> Vec<Message> {
        let mailbox = mailbox.trim().to_ascii_lowercase();
        let now = Utc::now();
        let mut inner = self.inner.write().await;
        prune_mailbox(
            &mut inner.by_mailbox,
            &mailbox,
            now,
            self.ttl,
            self.max_messages,
        );

        inner
            .by_mailbox
            .get(&mailbox)
            .map(|messages| messages.iter().rev().cloned().collect())
            .unwrap_or_default()
    }

    pub async fn get(&self, mailbox: &str, id: &str) -> Option<Message> {
        let mailbox = mailbox.trim().to_ascii_lowercase();
        let now = Utc::now();
        let mut inner = self.inner.write().await;
        prune_mailbox(
            &mut inner.by_mailbox,
            &mailbox,
            now,
            self.ttl,
            self.max_messages,
        );

        inner
            .by_mailbox
            .get(&mailbox)
            .and_then(|messages| messages.iter().rev().find(|item| item.id == id))
            .cloned()
    }

    pub async fn cleanup_expired(&self) -> usize {
        let now = Utc::now();
        let mut inner = self.inner.write().await;
        let keys: Vec<String> = inner.by_mailbox.keys().cloned().collect();
        let mut removed = 0;

        for mailbox in keys {
            let before = inner.by_mailbox.get(&mailbox).map_or(0, Vec::len);
            prune_mailbox(
                &mut inner.by_mailbox,
                &mailbox,
                now,
                self.ttl,
                self.max_messages,
            );
            let after = inner.by_mailbox.get(&mailbox).map_or(0, Vec::len);
            removed += before.saturating_sub(after);
        }

        removed
    }
}

impl Message {
    pub fn summary(&self) -> MessageSummary {
        let preview = build_preview(self.text.as_deref(), self.html.as_deref());
        MessageSummary {
            id: self.id.clone(),
            from: self.from.clone(),
            subject: self.subject.clone(),
            date: self.date,
            has_html: self
                .html
                .as_ref()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false),
            preview,
            received_at: self.received_at,
        }
    }
}

fn prune_mailbox(
    by_mailbox: &mut HashMap<String, Vec<Message>>,
    mailbox: &str,
    now: DateTime<Utc>,
    ttl: Duration,
    max_messages: usize,
) {
    let mut messages = match by_mailbox.remove(mailbox) {
        Some(value) => value,
        None => return,
    };

    let cutoff = now - ttl;
    messages.retain(|item| item.received_at >= cutoff);

    if messages.len() > max_messages {
        let keep_from = messages.len() - max_messages;
        messages.drain(0..keep_from);
    }

    if !messages.is_empty() {
        by_mailbox.insert(mailbox.to_string(), messages);
    }
}

fn build_preview(text: Option<&str>, html: Option<&str>) -> String {
    let mut source = text.unwrap_or_default().trim().to_string();
    if source.is_empty() {
        let html_source = html.unwrap_or_default();
        let tag_re = Regex::new(r"(?s)<[^>]*>").expect("valid html regex");
        source = tag_re.replace_all(html_source, " ").trim().to_string();
    }

    source = source.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = source.chars();
    let preview: String = chars.by_ref().take(120).collect();
    if chars.next().is_some() {
        format!("{}...", preview)
    } else {
        preview
    }
}
