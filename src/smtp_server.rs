use std::sync::Arc;

use chrono::Utc;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::address;
use crate::config::Config;
use crate::mail_parser;
use crate::store::{Message, Store};

#[derive(Clone)]
struct Recipient {
    mailbox: String,
    address: String,
}

#[derive(Default)]
struct Transaction {
    from: String,
    recipients: Vec<Recipient>,
}

impl Transaction {
    fn reset(&mut self) {
        self.from.clear();
        self.recipients.clear();
    }
}

pub async fn run(
    cfg: Arc<Config>,
    store: Store,
    mut shutdown_rx: watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let listen_addr = normalize_listen_addr(&cfg.smtp_addr);
    let listener = TcpListener::bind(&listen_addr).await?;
    info!("SMTP listening on {}", listen_addr);

    loop {
        tokio::select! {
            result = listener.accept() => {
                let (stream, peer) = result?;
                let cfg = cfg.clone();
                let store = store.clone();
                tokio::spawn(async move {
                    if let Err(err) = handle_connection(stream, cfg, store).await {
                        warn!("SMTP connection {} error: {}", peer, err);
                    }
                });
            }
            changed = shutdown_rx.changed() => {
                if changed.is_ok() && *shutdown_rx.borrow() {
                    info!("SMTP shutdown signal received");
                    break;
                }
            }
        }
    }

    Ok(())
}

async fn handle_connection(
    stream: TcpStream,
    cfg: Arc<Config>,
    store: Store,
) -> anyhow::Result<()> {
    let (reader_half, mut writer_half) = stream.into_split();
    let mut reader = BufReader::new(reader_half);
    let mut line = String::new();
    let mut tx = Transaction::default();
    let announce_domain = if cfg.domain.is_empty() {
        "localhost"
    } else {
        cfg.domain.as_str()
    };

    write_reply(
        &mut writer_half,
        format!("220 {} ESMTP ready\r\n", announce_domain).as_bytes(),
    )
    .await?;

    loop {
        line.clear();
        let read = reader.read_line(&mut line).await?;
        if read == 0 {
            break;
        }
        let input = line.trim_end_matches(['\r', '\n']);
        if input.is_empty() {
            continue;
        }

        let (verb, arg) = split_command(input);
        match verb.as_str() {
            "EHLO" => {
                let response = format!(
                    "250-{}\r\n250-SIZE {}\r\n250 8BITMIME\r\n",
                    announce_domain, cfg.max_message_bytes
                );
                write_reply(&mut writer_half, response.as_bytes()).await?;
            }
            "HELO" => {
                write_reply(
                    &mut writer_half,
                    format!("250 {}\r\n", announce_domain).as_bytes(),
                )
                .await?;
            }
            "MAIL" => match handle_mail_from(&cfg, &mut tx, arg) {
                Ok(_) => write_reply(&mut writer_half, b"250 OK\r\n").await?,
                Err((code, message)) => {
                    write_reply(
                        &mut writer_half,
                        format!("{} {}\r\n", code, message).as_bytes(),
                    )
                    .await?
                }
            },
            "RCPT" => match handle_rcpt_to(&cfg, &mut tx, arg) {
                Ok(_) => write_reply(&mut writer_half, b"250 OK\r\n").await?,
                Err((code, message)) => {
                    write_reply(
                        &mut writer_half,
                        format!("{} {}\r\n", code, message).as_bytes(),
                    )
                    .await?
                }
            },
            "DATA" => {
                if tx.recipients.is_empty() {
                    write_reply(&mut writer_half, b"554 no recipients\r\n").await?;
                    continue;
                }

                write_reply(&mut writer_half, b"354 End data with <CR><LF>.<CR><LF>\r\n").await?;

                match read_data_block(&mut reader, cfg.max_message_bytes).await {
                    Ok(raw_message) => match mail_parser::parse(&raw_message) {
                        Ok(parsed) => {
                            let now = Utc::now();
                            for rcpt in &tx.recipients {
                                let mut msg = Message {
                                    id: Uuid::new_v4().simple().to_string(),
                                    mailbox: rcpt.mailbox.clone(),
                                    to: rcpt.address.clone(),
                                    from: parsed.from.clone(),
                                    subject: parsed.subject.clone(),
                                    date: parsed.date,
                                    text: parsed.text.clone(),
                                    html: parsed.html.clone(),
                                    headers: parsed.headers.clone(),
                                    received_at: now,
                                };

                                if msg.from.trim().is_empty() {
                                    msg.from = tx.from.clone();
                                }
                                if msg.date.timestamp() == 0 {
                                    msg.date = now;
                                }

                                store.add(&rcpt.mailbox, msg).await;
                                info!(
                                    "mail received mailbox={} from={} subject={}",
                                    rcpt.mailbox, tx.from, parsed.subject
                                );
                            }
                            tx.reset();
                            write_reply(&mut writer_half, b"250 message accepted\r\n").await?;
                        }
                        Err(_) => {
                            tx.reset();
                            write_reply(&mut writer_half, b"550 invalid message content\r\n")
                                .await?;
                        }
                    },
                    Err((code, message)) => {
                        tx.reset();
                        write_reply(
                            &mut writer_half,
                            format!("{} {}\r\n", code, message).as_bytes(),
                        )
                        .await?;
                    }
                }
            }
            "RSET" => {
                tx.reset();
                write_reply(&mut writer_half, b"250 OK\r\n").await?;
            }
            "NOOP" => write_reply(&mut writer_half, b"250 OK\r\n").await?,
            "QUIT" => {
                write_reply(&mut writer_half, b"221 Bye\r\n").await?;
                break;
            }
            _ => {
                debug!("unknown SMTP command: {}", input);
                write_reply(&mut writer_half, b"500 command not recognized\r\n").await?;
            }
        }
    }

    Ok(())
}

fn handle_mail_from(cfg: &Config, tx: &mut Transaction, arg: &str) -> Result<(), (u16, String)> {
    let from = extract_smtp_address(arg, "FROM:").map_err(|msg| (550, msg))?;
    tx.recipients.clear();

    if from.is_empty() {
        tx.from.clear();
        return Ok(());
    }

    let (_, domain) =
        address::parse_email(&from).map_err(|_| (550, "invalid sender address".to_string()))?;
    if cfg.is_sender_domain_blocked(&domain) {
        return Err((530, "sender domain is blocked".to_string()));
    }

    tx.from = from.to_ascii_lowercase();
    Ok(())
}

fn handle_rcpt_to(cfg: &Config, tx: &mut Transaction, arg: &str) -> Result<(), (u16, String)> {
    let to = extract_smtp_address(arg, "TO:").map_err(|msg| (550, msg))?;
    let (mailbox, email_address) =
        address::normalize_mailbox(&to, &cfg.domain).map_err(|msg| (550, msg))?;

    if cfg.is_mailbox_blacklisted(&mailbox) {
        return Err((550, "mailbox is blocked".to_string()));
    }

    tx.recipients.push(Recipient {
        mailbox,
        address: email_address,
    });
    Ok(())
}

async fn read_data_block<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    max_message_bytes: usize,
) -> Result<Vec<u8>, (u16, String)> {
    let mut raw = Vec::new();
    let mut line = String::new();

    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .await
            .map_err(|_| (451, "failed to read message".to_string()))?;
        if read == 0 {
            return Err((451, "message terminated unexpectedly".to_string()));
        }

        if line == ".\r\n" || line == ".\n" || line == "." {
            break;
        }

        let mut bytes = std::mem::take(&mut line).into_bytes();
        if bytes.starts_with(b"..") {
            bytes.remove(0);
        }
        raw.extend_from_slice(&bytes);

        if raw.len() > max_message_bytes {
            return Err((552, "message too large".to_string()));
        }
    }

    Ok(raw)
}

fn split_command(input: &str) -> (String, &str) {
    let mut parts = input.splitn(2, ' ');
    let verb = parts.next().unwrap_or_default().trim().to_ascii_uppercase();
    let arg = parts.next().unwrap_or_default().trim();
    (verb, arg)
}

fn extract_smtp_address(arg: &str, prefix: &str) -> Result<String, String> {
    let upper = arg.to_ascii_uppercase();
    if !upper.starts_with(prefix) {
        return Err("invalid smtp path".to_string());
    }

    let raw = arg[prefix.len()..].trim();
    if raw.is_empty() {
        return Err("invalid smtp path".to_string());
    }

    let candidate = if let Some(rest) = raw.strip_prefix('<') {
        let close_idx = rest
            .find('>')
            .ok_or_else(|| "invalid smtp path".to_string())?;
        rest[..close_idx].trim().to_string()
    } else {
        raw.split_whitespace()
            .next()
            .unwrap_or_default()
            .trim()
            .to_string()
    };

    Ok(candidate)
}

async fn write_reply<W: AsyncWrite + Unpin>(writer: &mut W, reply: &[u8]) -> anyhow::Result<()> {
    writer.write_all(reply).await?;
    writer.flush().await?;
    Ok(())
}

fn normalize_listen_addr(addr: &str) -> String {
    if addr.starts_with(':') {
        format!("0.0.0.0{}", addr)
    } else {
        addr.to_string()
    }
}
