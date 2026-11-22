# Laftly runbook

What Casey does when something goes wrong: outages, safe mode, refunds,
disputes, complaints, STOP issues, the restore, key rotation, and the
incident and breach steps (`BUILD_BRIEF.md:526`). Every step is Casey's, on
his own computer or in the host and vendor dashboards. Claude never holds a
hosting key, so Claude never deploys, restores or rotates anything live; the
live service deploys only when Casey presses Deploy at Gate C (OPS-022).
Lines stay within 80 characters (`tests/docs/test_runbook.py`).

## 1. Host choice (OPS-012)

The app host is Railway Pro if its Postgres Backups tab shows point-in-time
recovery, otherwise Render (D-024). Check the tab before Gate C and note what
it showed in `docs/deploy.md`. Either way a restore lands in a new database
service, and the nightly encrypted export to separate storage stays, since
host backups fail with the host account (OPS-013).

## 2. Outages

1. The 5-minute health check pushes to Casey after it has failed for 15
   minutes (OPS-029). Open the host dashboard and read the `web` and
   `worker` deploy logs; `/healthz` answers only ok or 503 (OPS-007).
2. A crash at start that names a setting: fix the setting in the host's
   variables and redeploy. A start refused for index keys: section 9.
3. Database down or corrupt: turn on the charging pause first, then follow
   section 8 (restore).
4. A vendor down (Twilio, Stripe, the email provider): nothing to press. The
   worker retries; Stripe charges are never retried by the server (FEE-016).
   Note the outage window for the weekly digest.
5. Never roll back to a deployment from before a key rotation (section 9).

## 3. Safe mode (OPS-001, OPS-023, OPS-024)

Safe mode starts when a High alert that pushed to Casey's phone is still
unacknowledged 24 hours after its push. It pauses charges and releases and
nothing else: texts keep going, and safety texts, STOP, HELP and START
replies and pick codes are never paused.

- While safe mode is on, a "Confirm and pay" press is refused with a "try
  again later" message and charges nothing, and charges are refused until
  it ends. No charge waits to go ahead (FEE-016).
- A release waits while safe mode is on. When safe mode ends, each paused
  release goes ahead only after the FEE-017 check that the job is still
  waiting for it and the pick is still current (T408, T706, T707).
- To end it: acknowledge each High alert that started it on the owner
  home page, or let it acknowledge itself (OPS-023).

## 4. Refunds (FEE-026, FEE-029, OPS-019)

- Each automatic refund happens exactly once; a failed refund is retried
  hourly with an alert. A refund on a disputed charge is blocked and flagged.
- The nightly reconcile matches each unknown Stripe charge to a job or
  refunds it, always when the job is closed.
- A refund Casey decides: the owner portal's refund page for that pick. Read
  the evidence first; refunds are full, once, never on a disputed charge.

## 5. Disputes (FEE-027, FEE-050, OPS-005)

1. A bank dispute alerts Casey with evidence from the audit log; the ledger
   records the amount plus the Stripe dispute fee once.
2. The evidence pack goes to Stripe on its own 2 days before the due date
   only when FEE-050's condition holds; otherwise Casey decides to counter
   or accept in the owner portal before the due date.
3. Evidence carries IDs and timestamps only, no homeowner fields (PRIV-027).

## 6. Complaints (BIZ-011, BIZ-012, BIZ-029, OPS-005, OPS-036)

How a complaint reaches Casey:

- A homeowner complains by a PROBLEM text, a help-page message, an email to
  the help address or a reply to any Laftly email. Each becomes a help
  request or a complaint record (OPS-036, BIZ-029).
- A business complains through "Get help" or "Ask about a charge" in its
  portal, or by email reply; each opens a help request.
- Every complaint shows in the business portal with a reply box open for 5
  days, then is listed for Casey in the weekly digest with the reply.
- A complaint or PROBLEM reply with a safety word goes in the next 9 a.m. or
  3 p.m. push and needs Casey's decision (OPS-005, OPS-032).

What Casey does with it:

1. Read the complaint and the business's reply in the owner portal.
2. The homeowner already got an apology, the note that the business is
   independent, the business's own phone, the job details and the ROC
   complaint link (BIZ-011). Laftly is not a contractor and does not
   perform, bid on or supervise work, so Casey does not settle the work or
   the price.
3. A decision is needed only when the business has 2 or more complaints in
   90 days or a safety word was used. A business is paused only after 3
   complaints in 90 days (BIZ-012).
4. A safety word: call the homeowner through the owner call bridge and
   record the outcome in the owner portal.

## 7. STOP issues (CON-008 to CON-014, CON-035)

- A text that is only an opt-out word, English or Spanish (including
  "cancel"), opts the number out of all texts. "Cancel" alone never closes a
  job. A natural-language revocation is honored the same way (CON-010).
- After an opt-out nothing is sent to that number until it texts START,
  except the one confirmation (CON-009, CON-013). Error 21610 is never
  retried (CON-014).
- Someone says they opted out but still got a text: find the number's
  entry in the owner audit log; if it is missing, record it with the
  owner-recorded opt-out (`/owner/opt-out`), then turn on the texting pause
  if more than one number is affected and raise it with the developer.
- An opt-out by email, the help-page form or a phone call: every way feeds
  one suppression list, at once and never later than 10 business days
  (CON-035). A call is recorded with `/owner/opt-out`.

## 8. Restore (OPS-013 to OPS-015, OPS-051, PRIV-029)

The restore is the operator's, off the app host (D-4932). The host can write
an export but never read it back (PRIV-029) and never holds the private
export key (PRIV-024).

1. Turn on the charging and texting pauses if the owner portal still
   answers, then stop web and worker in the host dashboard and keep web
   stopped during the restore. Note the stop time. While web is stopped no
   opt-out, pick or press lands in the old database after the backup.
2. Create the new database: a point-in-time restore to a new service, or
   an empty PostgreSQL 16 service for an export.
3. On Casey's computer, fetch the export object with the storage read key
   the host never holds (PRIV-029). Kept 30 days (OPS-013).
4. Decrypt it with `age -d` and the escrowed private export key (PRIV-024):
   `age -d -i <escrowed key file> -o <plain dump file> <export object>`.
5. Run `scripts/bootstrap_roles.sql` against the new database, then make
   sure the operator shell's `VAULT_KEY` and `VAULT_INDEX_KEY` match the
   export (step 6) and set `ASKHAVA_MIGRATION_DATABASE_URL` to the fresh
   database's URL.
6. Restoring an export older than a rotation: put v1 back on the host
   lists, on both `VAULT_KEY` and `VAULT_INDEX_KEY`, before starting the
   worker or web, and in the operator shell for the replay. The rows in that
   export are under v1, and without it the start check refuses (section 9).
7. Run `server replay <plain dump file> <export object> <backup time>`. It
   runs `pg_restore --single-transaction` into the fresh database and then
   replays the lost window in time order from Twilio's sent log and Stripe's
   charges, sending nothing twice (OPS-015 to OPS-018). Confirm the
   ownership check (OPS-051): no object owned by a role other than the
   migration role, no `UPDATE` or `DELETE` for the app role on an
   append-only table.
8. Delete the plain dump file and the key file from the computer.
9. Switch web and worker to the restored database (OPS-014). Start web
   only, turn on the charging and texting pauses in the owner portal (the
   restored database holds the switches as they were at the backup), pass
   `/healthz`, then start the worker.
10. Opt-out check before ending the pause: `server replay` rebuilds only from
    Twilio's sent log and Stripe's charges (`app/server/src/lib.rs:1649`),
    never opt-outs (OPS-015, `docs/spec/ops-recovery.md:19`; CON-035). Read
    Twilio's inbound log from the backup time to now for opt-out words
    (CON-008 to CON-011), and the email and web opt-outs received in the
    same window (the email provider's inbound log, the help-page messages),
    and check each number against the suppression list. Record any missing
    one with `/owner/opt-out` before texting resumes.
11. If step 6 put v1 back: redo the rotation (section 9, steps 2 to 5),
    then drop v1.
12. Read the replay's list for Casey (OPS-019), then end the pauses. Record
    the drill or restore: snapshot time, restore time, data-loss window and
    the no-double-send and no-double-charge result.

The 30 days a retired key is kept restart after a restore (section 9).

## 9. Keys (PRIV-024, OPS-054)

### Key inventory

Every secret, with its owner, storage, offline escrow and rotation
trigger. Escrow is sealed on paper, kept apart from the password manager.
Live values are sealed host variables, never shown again by the host, so the
paper copy is the only recovery copy of `VAULT_KEY`.

| Secret | Owner | Storage | Escrow | Rotation trigger |
| --- | --- | --- | --- | --- |
| `VAULT_KEY` | Casey | sealed host var | paper | exposure; yearly |
| `VAULT_INDEX_KEY` | Casey | sealed host var | paper | with `VAULT_KEY` |
| `ID_SECRET` | Casey | sealed host var | paper | exposure |
| the photo-link key | Casey | `VAULT_KEY` | paper | with `VAULT_KEY` |
| the export key | Casey | off the host | paper | exposure; yearly |
| vendor keys | Casey | sealed host var | paper | exposure; staff change |

- `ID_SECRET` is the host variable `ASKHAVA_ID_SECRET`.
- The photo-link key has no variable of its own: photos are sealed under
  the `VAULT_KEY` versions and a link is a random token kept only as a hash,
  so it moves with `VAULT_KEY` (the photos batch of `server rotate-keys`).
- The export key: the host holds only the public recipient
  (`ASKHAVA_EXPORT_RECIPIENT`); the private half never sits on the host.
- Vendor keys: Twilio, Stripe, the email provider, the storage write key.
- Vault encryption is AES-256-GCM with a random 96-bit nonce from the OS and
  the key version in the associated data. Blind-index keys are derived per
  purpose (`homeowner-phone|v1`, `sender-dir|v1`, `email|v1`).

### Key format and the start check

`VAULT_KEY` and `VAULT_INDEX_KEY` are each a comma list of
`<version>:<base64 of 32 bytes>`, the highest version active
(`app/privacy/src/vault.rs:69` to `:71`), for example `2:<v2>,1:<v1>` during
a rotation.

The server and the worker run `check_index_keys` at start
(`app/server/src/main.rs:55`, `app/server/src/bin/worker.rs:46`,
`app/server/src/lib.rs:752` to `:767`) and refuse to start when contacts
hold phone digests only under a version no longer configured
(`app/privacy/src/vault.rs:985`) or opt-outs are held under an unconfigured
version (`app/messaging/src/suppression_versions.rs:39`). Put the missing
version back on the list; never delete rows to get past it.

### Rotation (`app/messaging/src/suppression_rekey.rs:6` to `:8`)

1. Write the new v2 keys on paper first, then add v2 to both lists,
   `VAULT_KEY` and `VAULT_INDEX_KEY`, and redeploy.
2. Run `server rotate-keys` until every batch is 0. It prints one line per
   batch: contacts and stated names, photos, staff phones and mailboxes,
   opt-outs, each with rows moved and rows skipped.
3. Run `server retire-index-version 1`.
4. If it refuses because v1 opt-outs have no v2 listing (a listing whose
   phone no contact holds, `app/messaging/src/suppression_rekey.rs:212` to
   `:214`): stop. Keep v1 on both lists, so the opt-outs stay honoured under
   it and the start check passes; note the count and ask the developer.
   Never delete those rows by hand.
5. Run `server rotate-keys` once more; it removes the stale v1 phone digests
   (`app/privacy/src/vault.rs:761` to `:762`).
6. Drop v1 from both lists only after a `rotate-keys` run in which all four
   batches printed 0 moved and 0 skipped and none was refused. Redeploy.

### Keeping retired keys (OPS-013)

Keep a retired key's paper copy, and the old export key, until 30 days after
the last export made under it (OPS-013, `docs/spec/ops-recovery.md:17`): a
kept export still holds rows under that key, and a restore of it needs the
key (section 8, step 6). The 30 days restart after a restore. Without the
key the restore fails, and a replay over a broken restore would resend
texts (OPS-015).

### Fix forward (OPS-054)

After a key rotation the fix goes forward: never roll the service back to a
deployment from before the rotation, since the host's rollback restores
that deployment's variables, and with them the old key list.

## 10. Breach and incident steps (OPS-020, OPS-021)

1. Pause texting and charging first (owner portal pause switches, OPS-003),
   and preserve the logs: export the host log lines now (the host keeps them
   30 days), and keep the audit stream and vendor logs.
2. Secret exposed: rotate it in the vendor, re-enter the sealed variable and
   redeploy. After a key rotation, fix forward and never roll back to a
   deployment from before it (OPS-054). A vault key: section 9.
3. Find what was exposed and whose data, from the audit log.
4. Arizona's 45-day notice (A.R.S. 18-552): notify each affected person
   within 45 days by letter, email or a live phone call, never by text
   alone.
5. Substitute notice where the law allows it: a letter to the Attorney
   General and a notice kept on laftly.com for at least 45 days.
6. Law-enforcement pause: if law enforcement says notice would hamper an
   investigation, wait while it says so, then notify.
7. Tell the Attorney General through the Attorney General's online notice
   form when the law requires it.
8. Each processor (Twilio, Stripe, the email provider, the host, storage)
   tells Laftly of a breach as soon as practicable; when one does, start at
   step 1.

## 11. Inbound mail retention (OPS-045, PRIV-026)

All inbound mail is received by the email provider and reaches the app only
through its signed webhook (D-4914). The inbound handler deletes each
received message from the provider after storing it, so no provider
retention period is needed. If that changes, record the provider's written
retention period here; it must be 7 days or less.
