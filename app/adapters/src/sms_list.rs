//! Listing Twilio's sent log for the replay after a restore (OPS-015, OPS-017; D-2764, D-3155,
//! D-3889).
//!
//! Twilio stores no custom key on a message, and its Messages list filters only by `To`, `From`
//! and `DateSent` (whole UTC days), so the replay never asks for the app's derived key: it lists
//! every message sent from one of the app's numbers over a window ([`SmsSentLog::list_sent`]:
//! `From`, `DateSent>` and `DateSent<`, following `next_page_uri`) and keeps those whose send time
//! falls inside it. The recipient and body come back as secrets; only `messaging` compares them
//! with its outbox rows (`messaging::api::replay_match`). A separate trait over [`SmsAdapter`],
//! so no existing texting fake changes (D-1753). A read, so it takes no idempotency key (D-596).

use std::future::Future;

use jiff::Timestamp;
use jiff::fmt::rfc2822::DateTimeParser;
use secrecy::SecretString;

use crate::base::{AdapterError, is_sid};
use crate::sms::{SmsAdapter, TwilioSms};

/// The most pages one listing follows (each up to [`SENT_LOG_PAGE_SIZE`] messages); a longer
/// chain is refused as [`AdapterError::Decode`] rather than followed for ever.
pub const SENT_LOG_MAX_PAGES: usize = 200;
/// Twilio's largest Messages page.
pub const SENT_LOG_PAGE_SIZE: &str = "1000";

/// One message in Twilio's sent log: its SID, `To`, `From`, body (`None` once redacted) and send
/// time. `To` and the body are secrets, so `Debug` prints neither (PRIV-016).
#[derive(Debug)]
pub struct SentMessage {
    /// The message SID (`SM…` or `MM…`).
    pub sid: String,
    /// The recipient's number; never logged or stored outside `messaging`.
    pub to: SecretString,
    /// The sending number.
    pub from: String,
    /// The text, `None` when Twilio holds none (redacted).
    pub body: Option<SecretString>,
    /// Twilio's `date_sent`.
    pub sent_at: Timestamp,
}

/// Listing the texts already sent (OPS-017, D-2764, D-3155).
pub trait SmsSentLog: SmsAdapter {
    /// Every message sent from `from` with a send time from `window.0` through `window.1`, all
    /// pages; a message with no send time yet is left out.
    fn list_sent(
        &self,
        from: &str,
        window: (Timestamp, Timestamp),
    ) -> impl Future<Output = Result<Vec<SentMessage>, AdapterError>> + Send;
}

/// One page of Twilio's Messages list; never logged.
#[derive(serde::Deserialize)]
struct MessagePage {
    messages: Vec<RawMessage>,
    next_page_uri: Option<String>,
}

/// The Message fields the replay needs; never logged.
#[derive(serde::Deserialize)]
struct RawMessage {
    sid: String,
    to: String,
    from: String,
    body: Option<String>,
    date_sent: Option<String>,
}

impl RawMessage {
    /// The message as listed; `None` before Twilio sent it. A malformed SID or date is `Decode`.
    fn sent(self) -> Result<Option<SentMessage>, AdapterError> {
        let Some(date) = self.date_sent else {
            return Ok(None);
        };
        if !(is_sid(&self.sid, "SM") || is_sid(&self.sid, "MM")) {
            return Err(AdapterError::Decode);
        }
        let sent_at = DateTimeParser::new()
            .parse_timestamp(&date)
            .map_err(|_| AdapterError::Decode)?;
        Ok(Some(SentMessage {
            sid: self.sid,
            to: SecretString::from(self.to),
            from: self.from,
            body: self.body.filter(|b| !b.is_empty()).map(SecretString::from),
            sent_at,
        }))
    }
}

impl SmsSentLog for TwilioSms {
    /// GET `Messages.json` with `From`, `DateSent>` and `DateSent<` (the window's UTC days) and
    /// `PageSize`, then each `next_page_uri` on the same host (SEC-032).
    async fn list_sent(
        &self,
        from: &str,
        (start, end): (Timestamp, Timestamp),
    ) -> Result<Vec<SentMessage>, AdapterError> {
        let (after, before) = (day(start), day(end));
        let first = format!("{}/Messages.json", self.http.account_url());
        let query = [
            ("From", from),
            ("DateSent>", after.as_str()),
            ("DateSent<", before.as_str()),
            ("PageSize", SENT_LOG_PAGE_SIZE),
        ];
        let mut page: MessagePage = self.http.get_json(&first, &query).await?;
        let mut out = Vec::new();
        for _ in 0..SENT_LOG_MAX_PAGES {
            for raw in page.messages {
                out.extend(
                    raw.sent()?
                        .filter(|m| start <= m.sent_at && m.sent_at <= end),
                );
            }
            let Some(next) = page.next_page_uri else {
                return Ok(out);
            };
            page = self.http.get_json(&self.http.page_url(&next)?, &[]).await?;
        }
        Err(AdapterError::Decode)
    }
}

/// A timestamp's UTC day, `YYYY-MM-DD`, as Twilio's `DateSent` filters take it.
fn day(at: Timestamp) -> String {
    at.strftime("%Y-%m-%d").to_string()
}

/// SEC-032, OPS-017 (D-3155): `next_page_uri` is followed only under this account's root, every
/// page is read, and the chain stops at [`SENT_LOG_MAX_PAGES`]. A local fake Twilio serves the
/// pages. Fake numbers only.
#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};

    use askhava_core::api::Settings;

    use super::*;
    use crate::api::{TwilioConfig, twilio_clients};

    const SID: &str = "AC00000000000000000000000000000000";
    const FROM: &str = "+15005550006";
    const ROOT: &str = "/2010-04-01/Accounts/AC00000000000000000000000000000000/";

    /// Twilio's clients with `base` as the API's base URL.
    fn sms(base: &str) -> TwilioSms {
        let pool: Vec<String> = (0..10).map(|i| format!("+1500555{:04}", 100 + i)).collect();
        let pairs: Vec<(String, String)> = [
            ("ASKHAVA_APP_ENV", "ci"),
            ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
            ("ASKHAVA_TWILIO_ACCOUNT_SID", SID),
            ("ASKHAVA_TWILIO_AUTH_TOKEN", "fake-auth-token"),
            (
                "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
                "VA00000000000000000000000000000001",
            ),
            ("ASKHAVA_MASKED_NUMBERS", &pool.join(",")),
            ("ASKHAVA_TWILIO_BASE_URL", base),
            ("ASKHAVA_TWILIO_VERIFY_BASE_URL", base),
        ]
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
        let settings = Settings::from_pairs(pairs.clone()).expect("settings");
        let config = TwilioConfig::from_settings(&settings, pairs).expect("config");
        twilio_clients(&config).expect("clients").0
    }

    /// A fake Twilio answering each request's path with `page(path)`; the paths it was asked
    /// for are kept. Its base URL is returned.
    fn fake(page: fn(&str) -> String, asked: Arc<Mutex<Vec<String>>>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                answer(stream.expect("stream"), page, &asked);
            }
        });
        format!("http://127.0.0.1:{port}/twilio")
    }

    /// Reads one request's head and answers it with `page(path)`, closing the connection.
    fn answer(mut stream: TcpStream, page: fn(&str) -> String, asked: &Mutex<Vec<String>>) {
        let mut head = Vec::new();
        let mut byte = [0_u8; 1];
        while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).expect("read") == 1 {
            head.push(byte[0]);
        }
        let line = String::from_utf8_lossy(&head);
        let path = line.split(' ').nth(1).unwrap_or_default().to_owned();
        let body = page(&path);
        asked.lock().expect("asked").push(path);
        let length = body.len();
        let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n{body}"
        );
        stream.write_all(reply.as_bytes()).expect("write");
    }

    /// One page holding fake message `n`, pointing to `next`.
    fn page_of(n: u32, next: Option<&str>) -> String {
        let next = next.map_or("null".to_owned(), |n| format!("\"{n}\""));
        format!(
            r#"{{"messages":[{{"sid":"SM{n:032}","to":"+15005550142","from":"{FROM}","body":"Fake","date_sent":"Thu, 19 Nov 2026 10:00:00 +0000"}}],"next_page_uri":{next}}}"#
        )
    }

    /// Page one points to page two, which ends the list.
    fn two_pages(path: &str) -> String {
        let next = format!("{ROOT}Messages.json?Page=1&PageToken=PAtwo");
        let last = path.contains("PageToken=PAtwo");
        page_of(if last { 2 } else { 1 }, (!last).then_some(next.as_str()))
    }

    fn window() -> (Timestamp, Timestamp) {
        let at = |s: &str| s.parse::<Timestamp>().expect("time");
        (at("2026-11-19T00:00:00Z"), at("2026-11-20T00:00:00Z"))
    }

    /// SEC-032: only a path under this account's root, without `..`, `\`, `#`, `@` or whitespace,
    /// becomes a URL, always on the configured host.
    #[test]
    fn page_url_takes_only_a_path_under_the_account() {
        let sms = sms("http://127.0.0.1:9/twilio");
        let good = format!("{ROOT}Messages.json?PageSize=1000&Page=1&PageToken=PAfake");
        let url = sms.http.page_url(&good).expect("good");
        assert_eq!(url, format!("http://127.0.0.1:9/twilio{good}"));
        let other = format!("AC{}", "1".repeat(32));
        let bad = [
            format!("https://evil.example{ROOT}Messages.json"),
            format!("//evil.example{ROOT}Messages.json"),
            format!("/2010-04-01/Accounts/{other}/Messages.json"),
            format!("{}extra/Messages.json", ROOT.trim_end_matches('/')),
            format!("{ROOT}../{other}/Messages.json"),
            format!("{ROOT}Messages.json?x=@evil.example"),
            format!("{ROOT}Messages.json#frag"),
            format!("{ROOT}Messages.json\\..\\x"),
            format!("{ROOT}Messages.json?x= y"),
            "Messages.json".to_owned(),
        ];
        for uri in bad {
            assert_eq!(sms.http.page_url(&uri), Err(AdapterError::Decode), "{uri}");
        }
    }

    /// OPS-017: every page is read through `next_page_uri`, on the configured host.
    #[tokio::test]
    async fn every_page_of_the_sent_log_is_listed() {
        let asked = Arc::default();
        let sms = sms(&fake(two_pages, Arc::clone(&asked)));
        let listed = sms.list_sent(FROM, window()).await.expect("listed");
        let sids: Vec<&str> = listed.iter().map(|m| m.sid.as_str()).collect();
        assert_eq!(sids, [format!("SM{:032}", 1), format!("SM{:032}", 2)]);
        let asked = asked.lock().expect("asked").clone();
        assert_eq!(asked.len(), 2);
        assert_eq!(
            asked[1],
            format!("/twilio{ROOT}Messages.json?Page=1&PageToken=PAtwo")
        );
    }

    /// SEC-032: a `next_page_uri` off the account is refused and never fetched.
    #[tokio::test]
    async fn an_off_account_next_page_is_refused() {
        fn page(_: &str) -> String {
            page_of(
                1,
                Some(
                    "http://evil.example/2010-04-01/Accounts/AC00000000000000000000000000000000/Messages.json",
                ),
            )
        }
        let asked = Arc::default();
        let sms = sms(&fake(page, Arc::clone(&asked)));
        let listed = sms.list_sent(FROM, window()).await;
        assert_eq!(listed.err(), Some(AdapterError::Decode));
        assert_eq!(asked.lock().expect("asked").len(), 1);
    }

    /// SEC-032: an endless chain stops after [`SENT_LOG_MAX_PAGES`] followed pages.
    #[tokio::test]
    async fn an_endless_page_chain_stops_at_the_cap() {
        fn page(_: &str) -> String {
            page_of(
                1,
                Some(&format!("{ROOT}Messages.json?Page=1&PageToken=PAagain")),
            )
        }
        let asked = Arc::default();
        let sms = sms(&fake(page, Arc::clone(&asked)));
        let listed = sms.list_sent(FROM, window()).await;
        assert_eq!(listed.err(), Some(AdapterError::Decode));
        let fetched = asked.lock().expect("asked").len();
        assert_eq!(
            fetched,
            SENT_LOG_MAX_PAGES + 1,
            "the first page and every followed one"
        );
    }
}
