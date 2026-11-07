//! Business notices by the business's chosen channels (BIZ-019, BIZ-036, BIZ-037, CARD-020,
//! TXT-014, TXT-035 to TXT-037, QH-005, PRIV-027, PICK-024; D-303, D-306 to D-308, D-491 to D-493,
//! D-1715, D-1760, D-1761).
//!
//! `messaging` never calls `businesses`. The enqueuing module (`quotes` for cards, `picks` for
//! pick, release and money notices, `businesses` for account notices) resolves the people through
//! `businesses::api::notice_recipients` (or the older `notice_addresses`) and queues one
//! [`BUSINESS_NOTICE_KIND`] task: `template` (the notice's words, the email's too), an optional
//! `text_template` (the text's words when they differ, as the release's), an optional
//! `business`, the `pick` or `job` it is about, `to` (opaque email IDs),
//! `texts` (`[{mailbox, phone}]`, phone line IDs, only when the business chose text) and `values`.
//! A job card adds `card` (the card text, no homeowner detail, CARD-005), `statement` (WORD-002)
//! and `page` (the signed-in job page; never image bytes, PRIV-027).
//!
//! [`business_notice`] holds the whole notice, email and text together, outside 8 a.m. to 8 p.m.
//! Arizona time by queueing itself again at 8 a.m. (QH-005). In the window `{time}` is set at the
//! send (D-1828): a card's `quote_hours` from now, a pick's "Accept by" from the composition's
//! [`SendChecks::accept_by`], a null time by the notice's `_untimed` variant. Each text candidate is
//! checked through [`SendChecks::may_text`] (`businesses::api::may_text`, read now, D-1715,
//! D-1760) and the business line's open sender (TXT-035); a refused phone, or a closed line, sends
//! that person the email instead (BIZ-019). A `{link}` is a per-recipient signed sign-in link
//! ([`sign_in_link`], D-1761); a notice with `{link}` and no job number fails, never falling back
//! to the shared portal address. [`card_notice_accepted`] is what `quotes`' sent count reads
//! (CARD-019, TD-TXT-22).
//!
//! Not here yet (D-1826): TXT-045's List-Unsubscribe and List-Unsubscribe-Post headers on card
//! emails (the email outbox stores no headers), the portal copy of each notice, Spanish business
//! emails (BIZ-018), the CARD-021 night emergency emails, and the send of a stored business text
//! (no business phone number is stored anywhere yet, D-492).

use std::future::Future;

use askama::Template;
use askhava_adapters::api::IdempotencyKey;
use askhava_core::api::{IdSecret, LinkKind, LinkSalt, NewTask, link_token};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::Value;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::email::{EMAIL_ACTION, MailboxKind, PostalAddress, Rendered, audience_kind};
use crate::mode::Lines;
use crate::models::{self, EmailError};
use crate::service::{NewText, Relation, enqueue};
use crate::templates::{Audience, Language, Registry};

/// The `core.task` kind of every business notice (D-491).
pub const BUSINESS_NOTICE_KIND: &str = "messaging.business_notice";
/// The job card's template (CARD-020): its words are the text and the email's first line.
pub const JOB_CARD_TEMPLATE: &str = "job_card_notice";
/// The business sign-in page every signed `{link}` opens (PICK-024, D-1761).
pub const SIGN_IN_URL: &str = "https://askhava.com/b/sign-in";
/// How long a signed sign-in link lasts (D-1761).
pub const LINK_LIFE: SignedDuration = SignedDuration::from_hours(7 * 24);
const PORTAL: &str = "https://askhava.com/b";
const ARIZONA: &str = "America/Phoenix";
/// The pick notices whose "Accept by" is set when they are sent (PICK-019, D-1503, D-1828).
const PICK_TEMPLATES: [&str; 2] = ["pick_notice", "pick_notice_emergency"];
/// The suffix of a notice's variant without `{time}`, used when its time is null: the card-fix
/// window stopped by the charging pause (FEE-020, FEE-038, D-1828).
const UNTIMED: &str = "_untimed";

/// The send-time checks the composition supplies, since `messaging` cannot call `businesses` or
/// `picks` (D-1715, D-1760, D-1828).
pub trait SendChecks: Send + Sync {
    /// CON-019: `businesses::api::may_text` for `phone`, a linked phone of `business`, now.
    fn may_text(
        &self,
        conn: &mut PgConnection,
        business: Uuid,
        phone: Uuid,
    ) -> impl Future<Output = bool> + Send;

    /// PICK-019, PICK-044, D-1503: the "Accept by" of pick `pick` when its notice is sent at
    /// `now`: `picks::api::notice_sent` (which starts the Confirm window there), or `now` plus
    /// 60 minutes for an emergency job; `None` when the pick moved on, and then nothing is sent.
    fn accept_by(
        &self,
        conn: &mut PgConnection,
        pick: Uuid,
        now: Timestamp,
    ) -> impl Future<Output = Option<Timestamp>> + Send;
}

/// The keys and the footer's postal line a business notice needs (SEC-028, SEC-018, SEC-037).
pub struct NoticeSender {
    secret: SecretString,
    links: IdSecret,
    postal: String,
}

impl NoticeSender {
    /// Checks `postal` as `kind` of mailbox before keeping it (SEC-028).
    ///
    /// # Errors
    /// [`EmailError::PostalAddress`] for a line that is not that kind of mailbox.
    pub fn new(
        (secret, links): (SecretString, IdSecret),
        postal: &str,
        kind: MailboxKind,
    ) -> Result<Self, EmailError> {
        PostalAddress::new(postal, kind)?;
        let postal = postal.trim().to_owned();
        Ok(Self {
            secret,
            links,
            postal,
        })
    }
}

/// What [`business_notice`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoticeOutcome {
    /// Outside the window: the whole notice was queued again for `until` (QH-005).
    Held {
        /// 8 a.m. Arizona time, when every channel goes together.
        until: Timestamp,
    },
    /// Nothing sent: the pick this notice asks to accept moved on before the send (D-1503).
    Dropped,
    /// Sent now: `emails` email rows and `texts` text rows were stored.
    Sent {
        /// Email rows stored (fallbacks included).
        emails: usize,
        /// Text rows stored.
        texts: usize,
    },
}

/// PICK-024, SEC-025, D-1761: the signed sign-in link of `person` for job `job`, valid until
/// `expires`: `core`'s Action token over the person, salted with the job number and the expiry
/// (8 big-endian bytes each), the three values beside it in the URL. One person, one link; `None`
/// only if the salt could not be built, and then no link is sent.
#[must_use]
pub fn sign_in_link(
    links: &IdSecret,
    person: Uuid,
    job: i64,
    expires: Timestamp,
) -> Option<String> {
    let bytes = [job.to_be_bytes(), expires.as_second().to_be_bytes()].concat();
    let salt = LinkSalt::from_bytes(&bytes).ok()?;
    let token = link_token(links, LinkKind::Action, person, &salt);
    Some(format!(
        "{SIGN_IN_URL}?p={person}&j={job}&e={}&t={}",
        expires.as_second(),
        token.as_str()
    ))
}

/// QH-005, QH-001: `None` from 8:00:00 to 19:59:59 Arizona time, else the next 8 a.m. there.
#[must_use]
pub fn held_until(now: Timestamp) -> Option<Timestamp> {
    let local = now.in_tz(ARIZONA).ok()?;
    if (8..20).contains(&local.hour()) {
        return None;
    }
    let day = if local.hour() < 8 {
        local.date()
    } else {
        local.date().tomorrow().ok()?
    };
    day.at(8, 0, 0, 0)
        .in_tz(ARIZONA)
        .ok()
        .map(|z| z.timestamp())
}

/// One parsed notice (R1): read once at the edge.
struct Parsed {
    template: String,
    text_template: String,
    business: Uuid,
    pick: Option<Uuid>,
    subject: Vec<Uuid>,
    job: Option<i64>,
    quote_hours: Option<i64>,
    to: Vec<Uuid>,
    texts: Vec<(Uuid, Uuid)>,
    values: Vec<(String, String)>,
    card: Option<[String; 3]>,
}

fn ids(v: &Value) -> Option<Vec<Uuid>> {
    let list = v.as_array().map_or(&[][..], Vec::as_slice);
    list.iter()
        .map(|x| x.as_str().and_then(|s| Uuid::parse_str(s).ok()))
        .collect()
}

fn uuid(v: &Value) -> Option<Uuid> {
    v.as_str().and_then(|s| Uuid::parse_str(s).ok())
}

/// The Arizona clock time of `seconds`, "4 p.m. Arizona time" or "4:30 p.m. Arizona time"
/// (TXT-033's style; business notices carry no second zone).
fn clock(seconds: i64) -> Option<String> {
    let t = Timestamp::from_second(seconds).ok()?.in_tz(ARIZONA).ok()?;
    let (h, m) = (t.hour(), t.minute());
    let half = if h < 12 { "a.m." } else { "p.m." };
    let h12 = if h % 12 == 0 { 12 } else { h % 12 };
    Some(match m {
        0 => format!("{h12} {half} Arizona time"),
        _ => format!("{h12}:{m:02} {half} Arizona time"),
    })
}

fn values(v: &Value) -> Vec<(String, String)> {
    let map = v.as_object().into_iter().flatten();
    map.filter_map(|(k, x)| {
        let s = match (k.as_str(), x) {
            ("time", Value::Number(n)) => clock(n.as_i64()?)?,
            (_, Value::String(s)) => s.clone(),
            (_, Value::Number(n)) => n.to_string(),
            _ => return None,
        };
        Some((k.clone(), s))
    })
    .collect()
}

fn parse(p: &Value) -> Option<Parsed> {
    let template = p["template"].as_str()?.to_owned();
    let business = uuid(&p["business"])?;
    let about = [uuid(&p["job"]), uuid(&p["pick"])];
    let texts = p["texts"].as_array().map_or(&[][..], Vec::as_slice);
    let texts = texts
        .iter()
        .map(|t| Some((uuid(&t["mailbox"])?, uuid(&t["phone"])?)))
        .collect::<Option<Vec<_>>>()?;
    let card = [&p["card"], &p["statement"], &p["page"]].map(|v| v.as_str().map(str::to_owned));
    let card = match card {
        [Some(a), Some(b), Some(c)] => Some([a, b, c]),
        _ => None,
    };
    Some(Parsed {
        text_template: p["text_template"].as_str().unwrap_or(&template).to_owned(),
        template,
        subject: std::iter::once(business)
            .chain(about.into_iter().flatten())
            .collect(),
        business,
        pick: uuid(&p["pick"]),
        job: p["values"]["job"].as_i64(),
        quote_hours: p["values"]["quote_hours"].as_i64(),
        to: ids(&p["to"])?,
        texts,
        values: values(&p["values"]),
        card,
    })
}

#[derive(Template)]
#[template(
    ext = "html",
    source = r#"{% extends "email_layout.html" %}{% block content %}
{% for line in lines %}<p>{{ line }}</p>
{% endfor %}{% if let Some(url) = link %}<p><a href="{{ url }}" style="color:#6B4A1E;text-decoration:underline;">{{ url }}</a></p>{% endif %}
{% endblock %}"#
)]
struct NoticeView<'a> {
    lang: &'a str,
    subject: &'a str,
    lines: &'a [String],
    link: Option<&'a str>,
    why: &'a str,
    manage_url: &'a str,
    manage_label: &'a str,
    postal: &'a str,
}

/// The words of `id` for `person`, `{link}` filled with their own signed link (D-1761); a
/// template that needs a value the notice lacks, `{link}` included, is an error.
fn words(n: &Parsed, id: &str, link: Option<&str>) -> Result<String, EmailError> {
    let mut pairs: Vec<(&str, &str)> = n
        .values
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    pairs.extend(link.map(|l| ("link", l)));
    let registry = Registry::embedded().map_err(|_| EmailError::Template)?;
    let text = registry.render(id, Language::En, false, &pairs);
    Ok(text.map_err(|_| EmailError::Template)?.body)
}

/// CARD-020: "Ask Hava: New <category> job in <town>. Quote by <time>." from the card's values.
fn card_subject(n: &Parsed) -> String {
    let value = |key: &str| {
        let found = n.values.iter().find(|(k, _)| k == key);
        found.map_or("", |(_, v)| v.as_str())
    };
    let (trade, town, time) = (value("trade"), value("town"), value("time"));
    format!("Ask Hava: New {trade} job in {town}. Quote by {time}.")
}

/// BIZ-019, CARD-020, PRIV-027, SEC-028: the email to `person`: the notice's words, its `{link}`
/// as a link of its own below them (and, for a card, the card, the WORD-002 statement and one link
/// to the signed-in job page; no image), in the layout with the business footer. Subject: the
/// card's CARD-020 subject, else the first sentence.
fn render(sender: &NoticeSender, n: &Parsed, link: Option<&str>) -> Result<Rendered, EmailError> {
    const MARK: &str = "\u{1}";
    let marked = words(n, &n.template, Some(MARK))?;
    if marked.contains(MARK) && link.is_none() {
        return Err(EmailError::Template);
    }
    let body = marked.replace(MARK, "").trim_end().to_owned();
    let subject = match &n.card {
        Some(_) => card_subject(n),
        None => body
            .split_inclusive(". ")
            .next()
            .unwrap_or(&body)
            .trim()
            .to_owned(),
    };
    let mut lines = vec![body];
    let mut page = link.filter(|_| marked.contains(MARK));
    if let Some([card, statement, url]) = &n.card {
        lines.extend([card.clone(), statement.clone()]);
        page = Some(url.as_str());
    }
    let why =
        "You get these emails because your business signed up for job requests from Ask Hava.";
    let view = NoticeView {
        lang: "en",
        subject: &subject,
        lines: &lines,
        link: page,
        why,
        manage_url: PORTAL,
        manage_label: "Manage email notices",
        postal: &sender.postal,
    };
    let html = view.render().map_err(|_| EmailError::Template)?;
    let page = page.map(|u| format!("{u}\n")).unwrap_or_default();
    let plain = format!(
        "{}\n{page}\n--\n{why}\nManage email notices: {PORTAL}\nAsk Hava LLC, {}\n",
        lines.join("\n"),
        sender.postal
    );
    Ok(Rendered {
        subject,
        html,
        plain,
    })
}

/// D-1761: `person`'s signed link, or none without a job number; a template with `{link}` then
/// fails rather than send the shared, unsigned portal address.
fn link_for(sender: &NoticeSender, n: &Parsed, person: Uuid, now: Timestamp) -> Option<String> {
    n.job
        .and_then(|job| sign_in_link(&sender.links, person, job, now + LINK_LIFE))
}

async fn store_email(
    db: &mut PgConnection,
    sender: &NoticeSender,
    n: &Parsed,
    (person, now): (Uuid, Timestamp),
) -> Result<(), EmailError> {
    let rendered = render(sender, n, link_for(sender, n, person, now).as_deref())?;
    let mut parts = vec![n.template.clone(), person.to_string()];
    parts.extend(n.subject.iter().map(Uuid::to_string));
    let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
    let key = IdempotencyKey::derive(&sender.secret, EMAIL_ACTION, &parts);
    let ids = (n.template.as_str(), person, n.subject.as_slice());
    let kind = audience_kind(Audience::Business);
    models::insert_email(db, (key.as_str(), ids, kind, false), &rendered, now).await?;
    Ok(())
}

/// Stores the texts the gate and the line allow and returns the people who get email instead.
async fn store_texts(
    db: &mut PgConnection,
    (sender, gate, lines): (&NoticeSender, &impl SendChecks, &Lines),
    n: &Parsed,
    now: Timestamp,
) -> Result<(usize, Vec<Uuid>), EmailError> {
    let (mut sent, mut fallback) = (0, Vec::new());
    let open = lines.line(Audience::Business).sender().is_some();
    for &(person, phone) in &n.texts {
        if !open || !gate.may_text(db, n.business, phone).await {
            fallback.push(person);
            continue;
        }
        let link = link_for(sender, n, person, now);
        let body = words(n, &n.text_template, link.as_deref())?;
        let text = NewText {
            template_id: &n.text_template,
            subject: &n.subject,
            recipient: phone,
            relation: Relation::Business,
            body: &body,
        };
        enqueue(db, &sender.secret, &text)
            .await
            .map_err(|_| EmailError::Database)?;
        sent += 1;
    }
    Ok((sent, fallback))
}

/// D-1828: fills `{time}` at the send. A card's `quote_hours` count from `now` (CARD-020: the
/// 24-hour window starts when the notice goes, so a night card held to 8 a.m. says 8 a.m. the next
/// day); a pick notice's "Accept by" is `checks.accept_by` (false: the pick moved on, send
/// nothing); a notice whose time is null uses its [`UNTIMED`] variant. A given time is kept.
async fn set_time(
    db: &mut PgConnection,
    checks: &impl SendChecks,
    n: &mut Parsed,
    now: Timestamp,
) -> Result<bool, EmailError> {
    if n.values.iter().any(|(k, _)| k == "time") {
        return Ok(true);
    }
    let due = if let Some(hours) = n.quote_hours {
        Some(now + SignedDuration::from_hours(hours))
    } else if PICK_TEMPLATES.contains(&n.template.as_str()) {
        let pick = n.pick.ok_or(EmailError::Template)?;
        match checks.accept_by(db, pick, now).await {
            Some(due) => Some(due),
            None => return Ok(false),
        }
    } else {
        None
    };
    match due.and_then(|t| clock(t.as_second())) {
        Some(time) => n.values.push(("time".to_owned(), time)),
        None => untimed(n)?,
    }
    Ok(true)
}

/// Switches a notice with no time to its [`UNTIMED`] variant, when it has one (FEE-038).
fn untimed(n: &mut Parsed) -> Result<(), EmailError> {
    let registry = Registry::embedded().map_err(|_| EmailError::Template)?;
    for id in [&mut n.template, &mut n.text_template] {
        let other = format!("{id}{UNTIMED}");
        if registry.template(&other, Language::En).is_some() {
            *id = other;
        }
    }
    Ok(())
}

/// The [`BUSINESS_NOTICE_KIND`] handler at `now` (see the module docs): held as a whole at night,
/// otherwise every text and email stored in one transaction; safe to run twice (JOB-006, SEC-018).
///
/// # Errors
/// [`EmailError::Template`] for a malformed payload or render, [`EmailError::Database`] otherwise.
pub async fn business_notice(
    db: &mut PgConnection,
    (sender, gate, lines): (&NoticeSender, &impl SendChecks, &Lines),
    (task_id, payload): (i64, &Value),
    now: Timestamp,
) -> Result<NoticeOutcome, EmailError> {
    let mut n = parse(payload).ok_or(EmailError::Template)?;
    if let Some(until) = held_until(now) {
        let mut again = NewTask::new(BUSINESS_NOTICE_KIND, payload.clone(), until);
        again.dedupe_key = Some(format!("{BUSINESS_NOTICE_KIND}.held.{task_id}"));
        askhava_core::api::enqueue(db, &again)
            .await
            .map_err(|_| EmailError::Database)?;
        return Ok(NoticeOutcome::Held { until });
    }
    if !set_time(db, gate, &mut n, now).await? {
        return Ok(NoticeOutcome::Dropped);
    }
    let (texts, fallback) = store_texts(db, (sender, gate, lines), &n, now).await?;
    let mut people = n.to.clone();
    people.extend(fallback.into_iter().filter(|p| !n.to.contains(p)));
    for &person in &people {
        store_email(db, sender, &n, (person, now)).await?;
    }
    Ok(NoticeOutcome::Sent {
        emails: people.len(),
        texts,
    })
}

/// CARD-019, TD-TXT-22: whether `business`'s card notice for `job` was accepted by the email
/// provider (sent, and not every such email hard-bounced) or by the texting provider.
///
/// # Errors
/// [`EmailError::Database`].
pub async fn card_notice_accepted(
    db: &mut PgConnection,
    job: Uuid,
    business: Uuid,
) -> Result<bool, EmailError> {
    sqlx::query_scalar!(
        "SELECT (EXISTS (SELECT 1 FROM messaging.email_outbox o WHERE o.template_id = $1 \
         AND o.subject_ids @> ARRAY[$2, $3]::uuid[] AND o.status = 'sent' AND NOT EXISTS \
         (SELECT 1 FROM messaging.email_event e WHERE e.provider_id = o.provider_id \
         AND e.kind = 'email.bounced')) OR EXISTS (SELECT 1 FROM messaging.outbox t \
         WHERE t.template_id = $1 AND t.subject_ids @> ARRAY[$2, $3]::uuid[] \
         AND t.status = 'sent')) AS \"yes!\"",
        JOB_CARD_TEMPLATE,
        job,
        business
    )
    .fetch_one(db)
    .await
    .map_err(|_| EmailError::Database)
}
