use once_cell::sync::Lazy;
use regex::Regex;

static MAILBOX_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-z0-9][a-z0-9._+\-]{0,63}$").expect("valid mailbox regex"));

pub fn parse_email(input: &str) -> Result<(String, String), String> {
    let value = normalize(input);
    let at = value
        .rfind('@')
        .ok_or_else(|| "invalid email address".to_string())?;
    if at == 0 || at == value.len() - 1 {
        return Err("invalid email address".to_string());
    }

    let mailbox = value[..at].trim().to_ascii_lowercase();
    let domain = value[at + 1..].trim().to_ascii_lowercase();

    validate_mailbox(&mailbox)?;
    if domain.is_empty() {
        return Err("invalid email domain".to_string());
    }

    Ok((mailbox, domain))
}

pub fn normalize_mailbox(input: &str, expected_domain: &str) -> Result<(String, String), String> {
    let value = normalize(input);
    let expected_domain = expected_domain.trim().to_ascii_lowercase();

    if value.contains('@') {
        let (mailbox, domain) = parse_email(&value)?;
        if !expected_domain.is_empty() && domain != expected_domain {
            return Err(format!("email domain must be {}", expected_domain));
        }
        return Ok((mailbox.clone(), format!("{}@{}", mailbox, domain)));
    }

    let mailbox = value.trim().to_ascii_lowercase();
    validate_mailbox(&mailbox)?;

    if expected_domain.is_empty() {
        Ok((mailbox.clone(), mailbox))
    } else {
        Ok((mailbox.clone(), format!("{}@{}", mailbox, expected_domain)))
    }
}

pub fn validate_mailbox(mailbox: &str) -> Result<(), String> {
    if !MAILBOX_PATTERN.is_match(mailbox) {
        return Err("invalid mailbox".to_string());
    }
    Ok(())
}

fn normalize(input: &str) -> String {
    input
        .trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .to_string()
}
