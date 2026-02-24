use std::collections::HashMap;

use chrono::{DateTime, Utc};
use mailparse::{self, ParsedMail};

#[derive(Debug, Clone)]
pub struct ParsedMessage {
    pub from: String,
    pub subject: String,
    pub date: DateTime<Utc>,
    pub text: Option<String>,
    pub html: Option<String>,
    pub headers: HashMap<String, Vec<String>>,
}

pub fn parse(raw: &[u8]) -> Result<ParsedMessage, String> {
    let parsed =
        mailparse::parse_mail(raw).map_err(|e| format!("failed to parse raw message: {e}"))?;
    let headers = extract_headers(&parsed);
    let from = find_first_header(&headers, "From").unwrap_or_default();
    let subject = find_first_header(&headers, "Subject").unwrap_or_default();
    let date = parse_date(find_first_header(&headers, "Date").as_deref());

    let mut text_parts = Vec::new();
    let mut html_parts = Vec::new();
    collect_body_parts(&parsed, &mut text_parts, &mut html_parts);

    let text = join_parts(text_parts);
    let html = join_parts(html_parts);

    Ok(ParsedMessage {
        from: from.trim().to_string(),
        subject: subject.trim().to_string(),
        date,
        text,
        html,
        headers,
    })
}

fn collect_body_parts(
    part: &ParsedMail<'_>,
    text_parts: &mut Vec<String>,
    html_parts: &mut Vec<String>,
) {
    if part.subparts.is_empty() {
        let content_type = part.ctype.mimetype.to_ascii_lowercase();
        if content_type == "text/plain" {
            if let Ok(body) = part.get_body() {
                if !body.trim().is_empty() {
                    text_parts.push(body);
                }
            }
        } else if content_type == "text/html" {
            if let Ok(body) = part.get_body() {
                if !body.trim().is_empty() {
                    html_parts.push(body);
                }
            }
        }
        return;
    }

    for subpart in &part.subparts {
        collect_body_parts(subpart, text_parts, html_parts);
    }
}

fn extract_headers(part: &ParsedMail<'_>) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for header in &part.headers {
        let key = header.get_key().to_string();
        let value = header.get_value();
        out.entry(key).or_default().push(value);
    }
    out
}

fn find_first_header(headers: &HashMap<String, Vec<String>>, key: &str) -> Option<String> {
    headers.iter().find_map(|(header_key, values)| {
        if header_key.eq_ignore_ascii_case(key) {
            values.first().cloned()
        } else {
            None
        }
    })
}

fn parse_date(date_header: Option<&str>) -> DateTime<Utc> {
    let now = Utc::now();
    let Some(date_str) = date_header else {
        return now;
    };

    match mailparse::dateparse(date_str) {
        Ok(timestamp) => DateTime::<Utc>::from_timestamp(timestamp, 0).unwrap_or(now),
        Err(_) => now,
    }
}

fn join_parts(parts: Vec<String>) -> Option<String> {
    let joined = parts
        .into_iter()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if joined.is_empty() {
        None
    } else {
        Some(joined)
    }
}
