//! The day-3 and day-14 check-ins by email in web-and-email mode (TXT-034's emailed-buttons
//! clause, SEC-026, JOB-015, T231, D-493, D-2414, D-2415).
//!
//! The email asks whether the work on the request is done and carries one button per answer, each
//! a plain link to the portals answer page `/c/<token>/<code>` ([`CHECK_IN_PREFIX`]), where the
//! token is the job's quote-page link token, issued by the caller in the transaction that queues
//! the email (SEC-037), and the code is `<day>-<answer>` ([`check_in_code`]). Opening a button
//! records nothing: the page shows the answer first and only its CSRF `POST` records it, so a mail
//! scanner's GET changes nothing. PROBLEM has no button: it needs her words, so the email asks her
//! to reply with what went wrong (D-2415). [`queue_check_in_email`] stores the email once in T094's
//! email outbox under an `email.send` key over the template, contact, job number and day; T094's
//! `send_email` sends it and reads her address from the vault then.

use askama::Template;
use askhava_adapters::api::IdempotencyKey;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::email::{
    EMAIL_ACTION, MailboxKind, OnDomain, PostalAddress, Rendered, audience_kind, footer_lines,
    legal_name, short_name,
};
use crate::models::{self, EmailError};
use crate::templates::{Audience, Language};

/// The check-in email's template ID; with the contact, job number and day it makes the key.
pub const CHECK_IN_EMAIL_TEMPLATE: &str = "check_in_email";
/// The answer page is this prefix, the link token, `/` and the answer code (TXT-034).
pub const CHECK_IN_PREFIX: OnDomain = OnDomain("/c/");
/// The two check-in days (JOB-015).
pub const CHECK_IN_DAYS: [u8; 2] = [3, 14];
/// The answers that have a button, in the email's order (JOB-015, JOB-028; PROBLEM by reply, D-2415).
pub const CHECK_IN_ANSWERS: [&str; 5] = ["done", "ongoing", "later", "no", "other"];
/// The homeowner's "Manage email notices" link (SEC-028).
const MANAGE: OnDomain = OnDomain("/h");

/// TXT-034: the answer code of `answer` on check-in day `day`, as the button's path ends.
#[must_use]
pub fn check_in_code(day: u8, answer: &str) -> String {
    format!("{day}-{answer}")
}

/// TXT-034: the day and answer of a code [`check_in_code`] made; `None` for any other text.
#[must_use]
pub fn parse_check_in_code(code: &str) -> Option<(u8, &'static str)> {
    let (day, answer) = code.split_once('-')?;
    let day = CHECK_IN_DAYS.into_iter().find(|d| d.to_string() == day)?;
    let answer = CHECK_IN_ANSWERS.into_iter().find(|a| *a == answer)?;
    Some((day, answer))
}

/// JOB-015: the words of `answer`'s button and of the answer page, in `language`.
#[must_use]
pub fn answer_label(language: Language, answer: &str) -> &'static str {
    let (en, es) = match answer {
        "done" => ("The work is done", "El trabajo está hecho"),
        "ongoing" => (
            "The regular service has started",
            "El servicio regular ya empezó",
        ),
        "later" => ("Not done yet", "Todavía no está hecho"),
        "no" => (
            "The business never contacted me",
            "El negocio nunca se comunicó conmigo",
        ),
        _ => ("I hired someone else", "Contraté a otra persona"),
    };
    if language == Language::Es { es } else { en }
}

/// The secret for email keys and Ask Hava LLC's checked postal line for the footer (SEC-028).
pub struct CheckInSender {
    secret: SecretString,
    postal: String,
}

impl CheckInSender {
    /// Checks `postal` as `kind` of mailbox before keeping it (SEC-028).
    ///
    /// # Errors
    /// [`EmailError::PostalAddress`] for a line that is not that kind of mailbox.
    pub fn new(secret: SecretString, postal: &str, kind: MailboxKind) -> Result<Self, EmailError> {
        PostalAddress::new(postal, kind)?;
        let postal = postal.trim().to_owned();
        Ok(Self { secret, postal })
    }
}

/// One check-in email: the job's request number and the check-in day (3 or 14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckInFacts {
    /// The request number she knows the job by.
    pub job_number: i64,
    /// 3 or 14 ([`CHECK_IN_DAYS`]).
    pub day: u8,
}

#[derive(Template)]
#[template(
    ext = "html",
    source = r#"{% extends "email_layout.html" %}{% block content %}
<p>{{ ask }}</p>
{% for (url, label) in buttons %}<p><a href="{{ url }}" style="display:inline-block;padding:12px 16px;border:2px solid #6B4A1E;border-radius:4px;color:#6B4A1E;text-decoration:underline;">{{ label }}</a></p>
{% endfor %}<p>{{ after }}</p>
<p>{{ problem }}</p>
{% endblock %}"#
)]
struct CheckInView<'a> {
    lang: &'a str,
    subject: &'a str,
    ask: &'a str,
    buttons: &'a [(String, &'static str)],
    after: &'static str,
    problem: &'static str,
    why: &'a str,
    manage_url: &'a str,
    manage_label: &'a str,
    legal: &'a str,
    postal: &'a str,
}

/// The subject, question, after-tap line and PROBLEM line in `language` (Spanish is a draft).
fn words(language: Language, job: i64) -> (String, String, [&'static str; 2]) {
    let short = short_name(language);
    match language {
        Language::En => (
            format!("{short}: Is the work on request {job} done?"),
            format!(
                "You picked a business for request {job}. Is the work done? Tap the answer that fits."
            ),
            [
                "A button only opens a page. You'll see your answer there, and nothing is saved until you press Send.",
                "If something went wrong, reply to this email and tell us what happened.",
            ],
        ),
        Language::Es => (
            format!("{short}: ¿Ya está hecho el trabajo de la solicitud {job}?"),
            format!(
                "Usted eligió un negocio para la solicitud {job}. ¿Ya está hecho el trabajo? Toque la respuesta que corresponde."
            ),
            [
                "Un botón solo abre una página. Ahí verá su respuesta, y no se guarda nada hasta que toque Enviar.",
                "Si algo salió mal, responda a este correo y cuéntenos qué pasó.",
            ],
        ),
    }
}

/// TXT-034, SEC-026, SEC-028: the check-in email of `facts` in `language` with one button per
/// answer, each linking `/c/<token>/<code>`; HTML and plain text carry the same lines and links.
///
/// # Errors
/// [`EmailError::Template`] if the layout fails to render.
pub fn render_check_in(
    sender: &CheckInSender,
    language: Language,
    facts: CheckInFacts,
    token: &str,
) -> Result<Rendered, EmailError> {
    let (subject, ask, [after, problem]) = words(language, facts.job_number);
    let buttons: Vec<(String, &'static str)> = CHECK_IN_ANSWERS
        .iter()
        .map(|a| {
            let code = check_in_code(facts.day, a);
            (
                format!("{CHECK_IN_PREFIX}{token}/{code}"),
                answer_label(language, a),
            )
        })
        .collect();
    let (why, manage_label) = footer_lines(Audience::Homeowner, language);
    let (manage, legal) = (MANAGE.to_string(), legal_name(language));
    let view = CheckInView {
        lang: if language == Language::Es { "es" } else { "en" },
        subject: &subject,
        ask: &ask,
        buttons: &buttons,
        after,
        problem,
        why: &why,
        manage_url: &manage,
        manage_label,
        legal,
        postal: &sender.postal,
    };
    let html = view.render().map_err(|_| EmailError::Template)?;
    let links: Vec<String> = buttons.iter().map(|(u, l)| format!("{l}: {u}")).collect();
    let plain = format!(
        "{ask}\n{}\n{after}\n{problem}\n\n--\n{why}\n{manage_label}: {manage}\n{legal}, {}\n",
        links.join("\n"),
        sender.postal
    );
    Ok(Rendered {
        subject,
        html,
        plain,
    })
}

/// TXT-034, SEC-018: stores the check-in email of `facts` for homeowner contact `contact` in
/// `language` once, in the caller's transaction (the one that issued `token`); `true` when new.
/// A second call with the same contact, job number and day stores nothing.
///
/// # Errors
/// [`EmailError::Template`] or a database error from the outbox insert.
pub async fn queue_check_in_email(
    db: &mut PgConnection,
    sender: &CheckInSender,
    (contact, language): (Uuid, Language),
    (facts, token): (CheckInFacts, &str),
    now: Timestamp,
) -> Result<bool, EmailError> {
    let rendered = render_check_in(sender, language, facts, token)?;
    let parts = [
        CHECK_IN_EMAIL_TEMPLATE.to_owned(),
        contact.to_string(),
        facts.job_number.to_string(),
        facts.day.to_string(),
    ];
    let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
    let key = IdempotencyKey::derive(&sender.secret, EMAIL_ACTION, &parts);
    let ids = (CHECK_IN_EMAIL_TEMPLATE, contact, &[][..]);
    let kind = audience_kind(Audience::Homeowner);
    let (_, new) =
        models::insert_email(db, (key.as_str(), ids, kind, false), &rendered, now).await?;
    Ok(new)
}

#[cfg(test)]
mod tests {
    //! TD-TXT-31 (TXT-034, SEC-026): one button per answer, each to its own answer page, the same
    //! links in both parts, no PROBLEM button, and one stored email per contact, job and day.
    //! Fake token and contact only.

    use secrecy::SecretString;
    use sqlx::types::Uuid;

    use super::{
        CHECK_IN_ANSWERS, CheckInFacts, CheckInSender, Language, MailboxKind, check_in_code,
        parse_check_in_code, queue_check_in_email, render_check_in,
    };

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn sender() -> CheckInSender {
        let secret = SecretString::from("fake-email-key-secret-0000000001".to_owned());
        CheckInSender::new(
            secret,
            "PO Box 1234, Lake Havasu City, AZ 86405",
            MailboxKind::PoBox,
        )
        .expect("sender")
    }

    const FACTS: CheckInFacts = CheckInFacts {
        job_number: 1042,
        day: 3,
    };

    #[test]
    fn td_txt_31_one_button_per_answer_each_to_its_answer_page() {
        for language in [Language::En, Language::Es] {
            let mail = render_check_in(&sender(), language, FACTS, TOKEN).expect("renders");
            for answer in CHECK_IN_ANSWERS {
                let link = format!("/c/{TOKEN}/3-{answer}\"");
                assert_eq!(mail.html.matches(&link).count(), 1, "{answer}");
                assert!(mail.plain.contains(&format!("/c/{TOKEN}/3-{answer}\n")));
            }
            assert_eq!(mail.html.matches("/c/").count(), CHECK_IN_ANSWERS.len());
            assert!(!mail.html.contains("problem\""), "no PROBLEM button");
            assert!(mail.subject.contains("1042"));
        }
    }

    #[test]
    fn td_txt_31_codes_round_trip_and_nothing_else_parses() {
        for day in [3, 14] {
            for answer in CHECK_IN_ANSWERS {
                let code = check_in_code(day, answer);
                assert_eq!(parse_check_in_code(&code), Some((day, answer)));
            }
        }
        for bad in ["3-problem", "7-done", "3done", "", "3-DONE", "14-done-x"] {
            assert_eq!(parse_check_in_code(bad), None, "{bad}");
        }
    }

    #[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
    async fn td_txt_31_one_email_per_contact_job_and_day(pool: sqlx::PgPool) {
        let mut db = pool.acquire().await.expect("db");
        let contact = Uuid::from_u128(0x0123_4567_89ab_cdef);
        let now = jiff::Timestamp::from_second(1_800_000_000).expect("time");
        let to = (contact, Language::En);
        let first = queue_check_in_email(&mut db, &sender(), to, (FACTS, TOKEN), now).await;
        assert!(first.expect("stored"));
        let again = queue_check_in_email(&mut db, &sender(), to, (FACTS, TOKEN), now).await;
        assert!(!again.expect("read"), "stored once");
        let day14 = CheckInFacts { day: 14, ..FACTS };
        let later = queue_check_in_email(&mut db, &sender(), to, (day14, TOKEN), now).await;
        assert!(later.expect("stored"));
    }
}
