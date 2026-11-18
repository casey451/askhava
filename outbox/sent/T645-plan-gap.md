---
id: T645-plan-gap
title: T645: plan gap reported, blocked for a plan fix
kind: blocked
---
T645's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/site/src/chat.rs:340 (owned by T593, then T646 and T649; T593 is not merged yet) calls `chat_message(db, &secret, &key, &msg, self.intake)` with no `Vault`, and the `ChatKeys` trait (chat.rs:62-66) only exposes secret/send/check. `messaging::api::suppress` (app/messaging/src/optout.rs:311-317) needs a `&Vault` and the phone, since digests come from `vault.blind_index` (optout.rs:441-443). `chat_message` (app/intake/src/web_chat.rs:247-253) takes no vault and its `Settings` is `Copy`, so T645 cannot list the session's number (step 1, TD-CON-36 "at once") without a signature change in chat.rs, which T645 does not own and is not ordered after T593 (D-4294, docs/decisions.md:2365, gives the vault hand-off to T649, which runs after T645).
Reviewer confirmed: T645 owns only web_chat.rs, optout.rs, migrations 0210/0211 and app/server/tests/web_chat_stops.rs (kit/tasks.json). No workaround in those files: privacy.contact_index holds digests but privacy::api exposes no vault-free lookup and optout.rs may not read privacy.* (docs/architecture.md:102); a Vault is only built in the server from secret keys (app/server/src/lib.rs:599, :646, :1465). Test-seam claim is partly overstated (stand-in Twilio mode exists, app/adapters/src/config.rs:110-113; a test can seed the session row), but the vault gap alone blocks.
Suggested fix: have the chat.rs/chat_codes.rs owner add `vault()` to `ChatKeys` and pass it to `chat_message` before T645 (order T645 after T593/T649's hand-off), or move the web-chat suppression cases of TD-CON-36 out of T645. Also note: `order::spanish_line` has no production caller, so open_verified must queue `spanish_not_yet` itself for TXT-041.
No files changed, no commits, no decision IDs used.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T645 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T645 --reason "..."` brings it back with the attempts it has left (D-2859).
