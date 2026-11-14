//! The business account event emails (SEC-024, BIZ-033, BIZ-040, BIZ-042, BIZ-007, BIZ-050;
//! T329, D-2287, D-2706, D-3112, D-3687).
//!
//! [`run_notice_email`] handles every kind in [`NOTICE_KINDS`]: it reads only the payload fields
//! that kind needs (any other field, such as T398's `number_set_at`, is ignored), picks the people
//! its spec line names, renders that kind's words in the person's language (BIZ-018) and queues
//! one email per recipient through `messaging`'s account email outbox (the SEC-028 footer and the
//! one sender), keyed by the template, the mailbox, the business and the task, so a retried or
//! repeated task stores and sends each email once. Recipients: `backup_changed`, `sample_card` and
//! `sample_pick` go to the payload's `to` mailboxes; `emergency_test` to the member's main address;
//! `notice_address_changed` to the `old` address (SEC-024); `notice_address_returned` to the
//! owner's main and backup addresses that may still get notices, never the returned one (BIZ-040,
//! D-3682); `phone_linked`, `after_hours_off` and `license_change` to the owner's account address.
//! Addresses, the person's language, the linking member's name, the phone's last 4 digits and
//! every link come through [`NoticeParts`], which the composition implements (`businesses` may not
//! call `privacy` or `portals`; the Approve link is `portals::api::mint_action_link`'s, D-3112).
//! The words of the four kinds that had none live here ([`address_changed_words`],
//! [`after_hours_off_words`], [`returned_words`], [`phone_linked_words`], D-3687; cold read and
//! copy panel, WORD-028, WORD-029). No address,
//! name, number or link is stored or logged here.

use std::future::Future;
use std::sync::Arc;

use askhava_adapters::api::EmailAdapter;
use askhava_core::api::{Claimed, HandlerError, arizona_time, brand};
use askhava_messaging::api::{EmailSent, EmailTo, queue_account_email, send_email};
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::account_emails::{AccountMailer, NO_ACCOUNT_SENDER};
use crate::backup_address::{BACKUP_CHANGED_KIND, backup_changed_words};
use crate::capability::{LICENSE_CHANGE_KIND, Reapplied, license_change_words};
use crate::complaints::{PHONE_LINKED_KIND, phone_linked_line};
use crate::deliverability_check::{
    SAMPLE_CARD_KIND, SAMPLE_PICK_KIND, sample_card_words, sample_pick_words,
};
use crate::email_verification::{
    CHANGED_KIND, RETURNED_KIND, may_get_notices, returned_line, returned_mailboxes,
};
use crate::emergency_switch::{
    AFTER_HOURS_OFF_KIND, EMERGENCY_TEST_KIND, OFF_REMOVED, OFF_SUSPENDED, emergency_test_words,
};
use crate::rules::JobTypeChoice;
use crate::team::{NoticePurpose, TeamMember, members};

/// Every `core.task` kind [`run_notice_email`] handles; the composition registers each one.
pub const NOTICE_KINDS: [&str; 9] = [
    BACKUP_CHANGED_KIND,
    PHONE_LINKED_KIND,
    SAMPLE_CARD_KIND,
    SAMPLE_PICK_KIND,
    CHANGED_KIND,
    RETURNED_KIND,
    EMERGENCY_TEST_KIND,
    AFTER_HOURS_OFF_KIND,
    LICENSE_CHANGE_KIND,
];
const FAILED: HandlerError = HandlerError("business event email failed");
const MALFORMED: HandlerError = HandlerError("business event email payload is malformed");

/// What the composition supplies for these emails: vault reads, the person's language and the
/// links `portals` makes. No value is stored or logged here.
pub trait NoticeParts: Send + Sync + 'static {
    /// The address of mailbox `mailbox`, if one is on file.
    fn address(
        &self,
        db: &mut PgConnection,
        mailbox: Uuid,
    ) -> impl Future<Output = Result<Option<SecretString>, HandlerError>> + Send;
    /// BIZ-018: whether team member `member` picked Spanish for business notices.
    fn spanish(
        &self,
        db: &mut PgConnection,
        member: Uuid,
    ) -> impl Future<Output = Result<bool, HandlerError>> + Send;
    /// The name of team member `member`, if known (BIZ-033).
    fn member_name(
        &self,
        db: &mut PgConnection,
        member: Uuid,
    ) -> impl Future<Output = Result<Option<String>, HandlerError>> + Send;
    /// The last 4 digits of phone line `business_line.1` of business `business_line.0`, if known.
    fn line_last4(
        &self,
        db: &mut PgConnection,
        business_line: (Uuid, Uuid),
    ) -> impl Future<Output = Result<Option<String>, HandlerError>> + Send;
    /// BIZ-033, BIZ-038, SEC-025: the owner `target.1`'s Approve action link for phone line
    /// `target.2` of business `target.0`, made at `now`.
    fn approve_link(
        &self,
        db: &mut PgConnection,
        target: (Uuid, Uuid, Uuid),
        now: Timestamp,
    ) -> impl Future<Output = Result<SecretString, HandlerError>> + Send;
    /// BIZ-007, BIZ-042: the link to the page that sends the test emergency email.
    fn retest_link(&self) -> String;
}

/// SEC-024, D-3687: the email telling the old address that the notice address changed, in English
/// or Spanish.
#[must_use]
pub fn address_changed_words(spanish: bool) -> [String; 2] {
    if spanish {
        let b = brand().es().short_name();
        return [
            format!("{b}: Cambió su correo para avisos"),
            "Alguien cambió el correo al que le mandamos sus avisos de trabajo. Esta dirección ya \
             no los recibirá. Si no fue usted, responda a este correo. Su respuesta nos llega \
             directamente."
                .to_owned(),
        ];
    }
    [
        format!("{}: Your notice email changed", brand().short_name()),
        "Someone changed the email address we send your job notices to. This address won't get \
         them anymore. If it wasn't you, reply to this email. Your reply comes straight to us."
            .to_owned(),
    ]
}

/// BIZ-007, D-3687: the owner's email when after-hours emergency requests stop: `suspended`
/// (unopened night EMERGENCY emails), or the tick removed because nobody who can confirm has a
/// tested "Emergency emails at any hour". Both end with `retest`, the page that sends the test.
#[must_use]
pub fn after_hours_off_words(spanish: bool, suspended: bool, retest: &str) -> [String; 2] {
    let (b, subject, body) = match (spanish, suspended) {
        (false, false) => (
            brand().short_name(),
            "After-hours emergency requests are off",
            "We turned off after-hours emergency requests for your business. No one on your team \
             who can confirm picks has \"Emergency emails at any hour\" turned on and tested. To \
             get them again, turn that on, send the test email and answer Yes. Then tick \
             after-hours emergencies again. Start here:",
        ),
        (false, true) => (
            brand().short_name(),
            "After-hours emergency requests are paused",
            "We paused after-hours emergency requests for your business. Your latest night \
             emergency emails weren't opened within an hour. Your phone may not be alerting you. \
             Send the test emergency email again. When you answer Yes, emergency requests start \
             again:",
        ),
        (true, false) => (
            brand().es().short_name(),
            "Las solicitudes de emergencia fuera de horario están desactivadas",
            "Desactivamos las solicitudes de emergencia fuera de horario para su negocio. Nadie \
             de su equipo que pueda confirmar trabajos tiene activado y probado «Correos de \
             emergencia a cualquier hora». Para recibirlas otra vez, actívelo, envíe el correo de \
             prueba y responda Sí. Luego vuelva a marcar emergencias fuera de horario. Empiece \
             aquí:",
        ),
        (true, true) => (
            brand().es().short_name(),
            "Las solicitudes de emergencia fuera de horario están en pausa",
            "Pausamos las solicitudes de emergencia fuera de horario para su negocio. Sus últimos \
             correos de emergencia de noche no se abrieron en menos de una hora. Puede que su \
             teléfono no le esté avisando. Envíe otra vez el correo de prueba de emergencia. \
             Cuando responda Sí, volverá a recibir solicitudes de emergencia:",
        ),
    };
    [format!("{b}: {subject}"), format!("{body} {retest}")]
}

/// BIZ-040, D-3687: the email to the owner's other addresses when `address` is returned: the
/// portal banner's sentence ([`returned_line`] in English) and the account link.
#[must_use]
pub fn returned_words(spanish: bool, address: &str) -> [String; 2] {
    let domain = brand().domain();
    if spanish {
        let es = brand().es();
        return [
            format!(
                "{}: Los correos a una de sus direcciones están rebotando",
                es.short_name()
            ),
            format!(
                "Los correos de {} a {address} se están devolviendo o se marcan como spam, así \
                 que podría perder solicitudes de trabajo. Entre en {domain}/b para agregar o \
                 corregir una dirección.",
                es.display_name()
            ),
        ];
    }
    [
        format!(
            "{}: Emails to one of your addresses are coming back",
            brand().short_name()
        ),
        format!("{} Sign in at {domain}/b.", returned_line(address)),
    ]
}

/// BIZ-033, CALL-012, D-3687: the owner's email after team member `member` linked the phone
/// ending `last4`: [`phone_linked_line`], then the Approve step with `approve`, the only action.
#[must_use]
pub fn phone_linked_words(
    spanish: bool,
    (member, last4): (&str, &str),
    approve: &str,
) -> [String; 2] {
    let line = phone_linked_line(member, last4, spanish);
    if spanish {
        return [
            format!(
                "{}: Un teléfono espera su aprobación",
                brand().es().short_name()
            ),
            format!(
                "{line}\n\nEse teléfono no funcionará con su cuenta hasta que usted lo apruebe. Para aprobarlo, toque aquí: {approve}"
            ),
        ];
    }
    [
        format!("{}: A phone needs your approval", brand().short_name()),
        format!(
            "{line}\n\nThat phone won't work with your account until you approve it. To approve it, tap here: {approve}"
        ),
    ]
}

/// One email to send: its template ID, the person whose language it uses, the mailboxes it goes
/// to, and its words once rendered.
struct Planned {
    template: &'static str,
    to: Vec<Uuid>,
    words: [String; 2],
}

fn id(task: &Claimed, field: &str) -> Result<Uuid, HandlerError> {
    task.payload[field]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or(MALFORMED)
}

fn ids(task: &Claimed, field: &str) -> Result<Vec<Uuid>, HandlerError> {
    let list = task.payload[field].as_array().ok_or(MALFORMED)?;
    list.iter()
        .map(|v| v.as_str().and_then(|s| Uuid::parse_str(s).ok()))
        .collect::<Option<Vec<_>>>()
        .ok_or(MALFORMED)
}

/// SEC-024, BIZ-040, BIZ-033, BIZ-042, BIZ-007, BIZ-050, SEC-018: sends `task`'s email to each of
/// its recipients once; a kind not in [`NOTICE_KINDS`] or a malformed payload is an error.
///
/// # Errors
/// [`NO_ACCOUNT_SENDER`] without a sender; a fixed sentence for a malformed payload, a missing
/// address, name or digits, a held email or a failed read, store or send (the queue tries again).
pub async fn run_notice_email<E: EmailAdapter + Send + Sync, P: NoticeParts>(
    pool: PgPool,
    mailer: Option<Arc<AccountMailer<E, P>>>,
    task: Claimed,
) -> Result<(), HandlerError> {
    let business = id(&task, "business")?;
    let mailer = mailer.ok_or(HandlerError(NO_ACCOUNT_SENDER))?;
    let mut db = pool.acquire().await.map_err(|_| FAILED)?;
    let team = members(&mut db, business).await.map_err(|_| FAILED)?;
    let Some(owner) = team.iter().find(|m| m.owner).copied() else {
        return Err(HandlerError("the business has no owner"));
    };
    let Some(planned) = plan(&mut db, &mailer, (&task, &team, owner)).await? else {
        return Ok(());
    };
    let task_id = Uuid::from_u128(u128::from(task.id.unsigned_abs()));
    for mailbox in &planned.to {
        let email = (planned.template, [business, task_id], &planned.words);
        send(&mut db, &mailer, *mailbox, email).await?;
    }
    Ok(())
}

/// The email `task` asks for, or `None` when its person has left and nobody is to be told.
async fn plan<E, P: NoticeParts>(
    db: &mut PgConnection,
    mailer: &AccountMailer<E, P>,
    (task, team, owner): (&Claimed, &[TeamMember], TeamMember),
) -> Result<Option<Planned>, HandlerError> {
    let parts = &mailer.directory;
    let kind = task.kind.as_str();
    let person = match kind {
        BACKUP_CHANGED_KIND | SAMPLE_CARD_KIND | SAMPLE_PICK_KIND | CHANGED_KIND
        | EMERGENCY_TEST_KIND => id(task, "member")?,
        _ => owner.id,
    };
    let spanish = parts.spanish(db, person).await?;
    let (template, to, words) = match kind {
        EMERGENCY_TEST_KIND => {
            let Some(m) = team.iter().find(|m| m.id == person) else {
                return Ok(None);
            };
            let words = emergency_test_words(spanish);
            ("business_emergency_test", vec![m.mailbox], words)
        }
        RETURNED_KIND => return returned(db, parts, (task, owner), spanish).await,
        AFTER_HOURS_OFF_KIND => {
            let words = after_hours(task, parts, spanish)?;
            ("business_after_hours_off", vec![owner.mailbox], words)
        }
        PHONE_LINKED_KIND => {
            let words = linked(db, mailer, (task, owner), spanish).await?;
            ("business_phone_linked", vec![owner.mailbox], words)
        }
        LICENSE_CHANGE_KIND => {
            let words = license(task, spanish)?;
            ("business_license_change", vec![owner.mailbox], words)
        }
        _ => listed(task, spanish)?,
    };
    Ok(Some(Planned {
        template,
        to,
        words,
    }))
}

/// The kinds whose payload names their mailboxes (`to`, or the `old` address, SEC-024).
fn listed(
    task: &Claimed,
    spanish: bool,
) -> Result<(&'static str, Vec<Uuid>, [String; 2]), HandlerError> {
    Ok(match task.kind.as_str() {
        BACKUP_CHANGED_KIND => {
            let words = backup_changed_words(spanish);
            ("business_backup_changed", ids(task, "to")?, words)
        }
        SAMPLE_CARD_KIND => {
            let words = sample_card_words(spanish);
            ("business_sample_card", ids(task, "to")?, words)
        }
        SAMPLE_PICK_KIND => {
            let words = sample_pick(task, spanish)?;
            ("business_sample_pick", ids(task, "to")?, words)
        }
        CHANGED_KIND => {
            let words = address_changed_words(spanish);
            ("business_address_changed", vec![id(task, "old")?], words)
        }
        _ => return Err(HandlerError("not a business event email kind")),
    })
}

/// BIZ-042: the sample pick's words with its `confirm_by` (Unix seconds) as Arizona clock time.
fn sample_pick(task: &Claimed, spanish: bool) -> Result<[String; 2], HandlerError> {
    let at = task.payload["confirm_by"].as_i64().ok_or(MALFORMED)?;
    let at = Timestamp::from_second(at).map_err(|_| MALFORMED)?;
    let time = arizona_time(at).strftime("%-I:%M %p").to_string();
    Ok(sample_pick_words(&time, spanish))
}

/// BIZ-007: the after-hours words for the payload's `reason`.
fn after_hours<P: NoticeParts>(
    task: &Claimed,
    parts: &P,
    spanish: bool,
) -> Result<[String; 2], HandlerError> {
    match task.payload["reason"].as_str() {
        Some(OFF_REMOVED) => Ok(after_hours_off_words(spanish, false, &parts.retest_link())),
        Some(OFF_SUSPENDED) => Ok(after_hours_off_words(spanish, true, &parts.retest_link())),
        _ => Err(MALFORMED),
    }
}

/// BIZ-040, D-3682: the returned-address email to the owner's main address (while it may get
/// notices) and the owner's verified backup, never to a returned mailbox; `None` when no such
/// address is left.
async fn returned<P: NoticeParts>(
    db: &mut PgConnection,
    parts: &P,
    (task, owner): (&Claimed, TeamMember),
    spanish: bool,
) -> Result<Option<Planned>, HandlerError> {
    let (business, gone) = (owner.business, id(task, "mailbox")?);
    let bad = returned_mailboxes(db, business).await.map_err(|_| FAILED)?;
    let mut to = Vec::new();
    for mailbox in owner_addresses(db, owner).await? {
        let main_ok = mailbox != owner.mailbox
            || may_get_notices(db, business, mailbox)
                .await
                .map_err(|_| FAILED)?;
        if mailbox != gone && !bad.contains(&mailbox) && main_ok {
            to.push(mailbox);
        }
    }
    if to.is_empty() {
        return Ok(None);
    }
    let address = parts.address(db, gone).await?;
    let address = address.ok_or(HandlerError("the returned address is not on file"))?;
    let words = returned_words(spanish, address.expose_secret());
    Ok(Some(Planned {
        template: "business_address_returned",
        to,
        words,
    }))
}

/// The owner's main mailbox and, when verified, the owner's backup: the backup follows its main
/// in [`crate::backup_address::notice_addresses`], before any other member's main mailbox.
async fn owner_addresses(
    db: &mut PgConnection,
    owner: TeamMember,
) -> Result<Vec<Uuid>, HandlerError> {
    let business = owner.business;
    let all = crate::backup_address::notice_addresses(db, business, NoticePurpose::Cards)
        .await
        .map_err(|_| FAILED)?;
    let mains: Vec<Uuid> = members(db, business)
        .await
        .map_err(|_| FAILED)?
        .iter()
        .map(|m| m.mailbox)
        .collect();
    let from = all
        .iter()
        .position(|m| *m == owner.mailbox)
        .unwrap_or(all.len());
    let backups = all.iter().skip(from + 1).take_while(|m| !mains.contains(m));
    Ok(std::iter::once(owner.mailbox)
        .chain(backups.copied())
        .collect())
}

/// BIZ-033: the phone-linked words with the member's name, the phone's last 4 digits and the
/// owner's Approve link.
async fn linked<E, P: NoticeParts>(
    db: &mut PgConnection,
    mailer: &AccountMailer<E, P>,
    (task, owner): (&Claimed, TeamMember),
    spanish: bool,
) -> Result<[String; 2], HandlerError> {
    let parts = &mailer.directory;
    let (member, line) = (id(task, "member")?, id(task, "line")?);
    let name = parts.member_name(db, member).await?;
    let name = name.ok_or(HandlerError("the linking member's name is not known"))?;
    let last4 = parts.line_last4(db, (owner.business, line)).await?;
    let last4 = last4
        .filter(|d| d.len() == 4 && d.bytes().all(|b| b.is_ascii_digit()))
        .ok_or(HandlerError(
            "the linked phone's last 4 digits are not known",
        ))?;
    let now = mailer.clock.now();
    let link = parts
        .approve_link(db, (owner.business, owner.id, line), now)
        .await?;
    Ok(phone_linked_words(
        spanish,
        (&name, &last4),
        link.expose_secret(),
    ))
}

/// BIZ-050, D-2596: the license-change words from the payload's job type pairs, reason and pause.
fn license(task: &Claimed, spanish: bool) -> Result<[String; 2], HandlerError> {
    let pairs = |field: &str| -> Result<Vec<JobTypeChoice>, HandlerError> {
        let list = task.payload[field].as_array().ok_or(MALFORMED)?;
        list.iter()
            .map(|p| match (p[0].as_str(), p[1].as_str()) {
                (Some(c), Some(t)) => Ok(JobTypeChoice::new(c, t)),
                _ => Err(MALFORMED),
            })
            .collect()
    };
    let dropped = task.payload["dropped_classes"]
        .as_array()
        .ok_or(MALFORMED)?;
    let dropped: Vec<&str> = dropped.iter().filter_map(Value::as_str).collect();
    let change = Reapplied {
        removed: pairs("removed")?,
        offered: pairs("offered")?,
        paused: task.payload["paused"].as_bool().ok_or(MALFORMED)?,
    };
    Ok(license_change_words(spanish, &change, &dropped))
}

/// Queues `words` to `mailbox` and sends it; `Ok` once the provider took it now or earlier
/// (SEC-018: one row and one provider key per template, mailbox, business and task).
async fn send<E: EmailAdapter + Send + Sync, P: NoticeParts>(
    db: &mut PgConnection,
    mailer: &AccountMailer<E, P>,
    mailbox: Uuid,
    (template, subject_ids, [subject, body]): (&str, [Uuid; 2], &[String; 2]),
) -> Result<(), HandlerError> {
    let to = mailer.directory.address(db, mailbox).await?;
    let to = to.ok_or(HandlerError("a recipient's address is not on file"))?;
    let now = mailer.clock.now();
    let (postal, kind) = (mailer.postal.0.as_str(), mailer.postal.1);
    let lines: Vec<String> = body.split("\n\n").map(str::to_owned).collect();
    let ids = (template, mailbox, subject_ids.as_slice());
    let (row, _) = queue_account_email(
        db,
        (&mailer.secret, (postal, kind)),
        ids,
        (subject, &lines),
        now,
    )
    .await
    .map_err(|_| FAILED)?;
    let keys = (&mailer.secret, &mailer.provider, mailer.limit);
    match send_email(db, keys, row, EmailTo::Business(to.expose_secret()), now).await {
        Ok(EmailSent::Sent(_) | EmailSent::AlreadySent) => Ok(()),
        Ok(EmailSent::Waiting(_)) => Err(HandlerError("business event email is held")),
        Err(_) => Err(FAILED),
    }
}
