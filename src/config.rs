use std::collections::HashSet;
use std::env;

const DEFAULT_MAILBOX_BLACKLIST: &[&str] = &[
    "admin",
    "master",
    "info",
    "mail",
    "webadmin",
    "webmaster",
    "noreply",
    "system",
    "postmaster",
];

#[derive(Debug, Clone)]
pub struct Config {
    pub http_addr: String,
    pub smtp_addr: String,
    pub domain: String,
    pub mailbox_blacklist: HashSet<String>,
    pub banned_sender_domains: HashSet<String>,
    pub max_messages_per_mailbox: usize,
    pub message_ttl_minutes: i64,
    pub max_message_bytes: usize,
}

impl Config {
    pub fn load() -> Self {
        let http_addr = getenv_default("HTTP_ADDR", ":3000");
        let smtp_addr = getenv_default("SMTP_ADDR", ":25");
        let domain = normalize_domain(&env::var("MAIL_DOMAIN").unwrap_or_default());

        let mailbox_blacklist = parse_list_env("MAILBOX_BLACKLIST").unwrap_or_else(|| {
            DEFAULT_MAILBOX_BLACKLIST
                .iter()
                .map(|x| x.to_string())
                .collect()
        });
        let banned_sender_domains = parse_list_env("BANNED_SENDER_DOMAINS").unwrap_or_default();

        let max_messages_per_mailbox = parse_usize_env("MAX_MESSAGES_PER_MAILBOX", 200).max(1);
        let message_ttl_minutes = parse_i64_env("MESSAGE_TTL_MINUTES", 1440).max(1);
        let max_message_bytes = parse_usize_env("MAX_MESSAGE_BYTES", 10 * 1024 * 1024).max(1024);

        Self {
            http_addr,
            smtp_addr,
            domain,
            mailbox_blacklist,
            banned_sender_domains,
            max_messages_per_mailbox,
            message_ttl_minutes,
            max_message_bytes,
        }
    }

    pub fn is_mailbox_blacklisted(&self, mailbox: &str) -> bool {
        self.mailbox_blacklist
            .contains(&mailbox.trim().to_ascii_lowercase())
    }

    pub fn is_sender_domain_blocked(&self, domain: &str) -> bool {
        self.banned_sender_domains
            .contains(&domain.trim().to_ascii_lowercase())
    }
}

fn getenv_default(key: &str, fallback: &str) -> String {
    let value = env::var(key).unwrap_or_default();
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn normalize_domain(domain: &str) -> String {
    domain.trim().to_ascii_lowercase()
}

fn parse_usize_env(key: &str, fallback: usize) -> usize {
    env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(fallback)
}

fn parse_i64_env(key: &str, fallback: i64) -> i64 {
    env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(fallback)
}

fn parse_list_env(key: &str) -> Option<HashSet<String>> {
    let value = env::var(key).ok()?;
    let mut out = HashSet::new();
    for item in value.split(',') {
        let normalized = item.trim().to_ascii_lowercase();
        if !normalized.is_empty() {
            out.insert(normalized);
        }
    }
    Some(out)
}
