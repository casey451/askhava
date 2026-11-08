# Mail-app steps (BIZ-042)

The words of every mail-app step on the sign-up inbox check and the "Send a sample pick" step.
`app/portals/src/email_check.rs` reads this file at build time (`steps`), so the screens show
exactly these rows. Each row names the app (`gmail`, `outlook`, `yahoo`, `iphone`, `other`), the
step, the maker's help page it was checked against and the date it was checked (D-328, D-352,
D-380). A row for "another mail app" has no maker, so its page is `none`.

To change a step: open the maker's page, check the step against it, then change the row and its
date in the same edit. Keep each step short and in the reader's words (`docs/voice.md`).

## inbox

Shown after the answer Spam or Junk, Promotions or another tab, or Not yet, with a resend.

| app | step | help page | checked |
|---|---|---|---|
| gmail | Open Spam, open our sample email and tap Not spam. | https://support.google.com/mail/answer/1366858 | 2026-11-08 |
| gmail | Add jobs@notify.askhava.com to your contacts. | https://support.google.com/mail/answer/16457426 | 2026-11-08 |
| gmail | Make a filter for mail from jobs@notify.askhava.com and tick "Never send it to Spam". | https://support.google.com/mail/answer/16457426 | 2026-11-08 |
| gmail | If it landed in Promotions or Updates, drag it to the Primary tab. | https://support.google.com/mail/answer/3055016 | 2026-11-08 |
| outlook | Open Junk Email, open our sample email and choose Not junk. | https://support.microsoft.com/en-us/outlook/mail-goes-to-the-junk-folder-by-mistake | 2026-11-08 |
| outlook | Add jobs@notify.askhava.com to your Safe senders. | https://support.microsoft.com/en-us/outlook/mail-goes-to-the-junk-folder-by-mistake | 2026-11-08 |
| yahoo | Open Spam, open our sample email and click Not spam. | https://help.yahoo.com/kb/SLN28056.html | 2026-11-08 |
| iphone | Open our sample email, tap our name at the top and tap Add to VIP. | https://support.apple.com/en-us/104971 | 2026-11-08 |
| other | Add jobs@notify.askhava.com to your contacts. | none | 2026-11-08 |

## last-step

The picture guide under "Reply OK to this email, star it and add us to your contacts".

| app | step | help page | checked |
|---|---|---|---|
| gmail | Reply OK to the sample and star it. | https://support.google.com/mail/answer/5904 | 2026-11-08 |
| gmail | Add jobs@notify.askhava.com to your contacts. | https://support.google.com/mail/answer/16457426 | 2026-11-08 |
| gmail | Move it to the Primary tab. By default, the Gmail app only alerts you for mail in Primary. | https://support.google.com/mail/answer/1075549 | 2026-11-08 |
| outlook | Reply OK and add jobs@notify.askhava.com to your Safe senders. | https://support.microsoft.com/en-us/outlook/mail-goes-to-the-junk-folder-by-mistake | 2026-11-08 |
| outlook | If you use Focused Inbox, move our email to Focused and choose Always move to Focused. | https://support.microsoft.com/en-us/outlook/mail/focused-inbox-for-outlook | 2026-11-08 |
| yahoo | Reply OK and add us to your contacts from the sender's contact card. | https://help.yahoo.com/kb/add-edit-delete-contacts-yahoo-mail-sln28059.html | 2026-11-08 |
| iphone | Reply OK, add us to VIP, then in Settings turn on VIP alerts for Mail. | https://support.apple.com/en-us/104971 | 2026-11-08 |
| other | Reply OK and add jobs@notify.askhava.com to your contacts. | none | 2026-11-08 |

## pick-alert

Shown on "Send a sample pick" after a No to "Did your phone alert you?".

| app | step | help page | checked |
|---|---|---|---|
| iphone | Add us to VIP, turn on VIP alerts and give them a sound of their own. | https://support.apple.com/en-us/104971 | 2026-11-08 |
| iphone | In Settings, open Focus. In Do Not Disturb and in Sleep, allow Mail or add us as an allowed person. | https://support.apple.com/en-us/108274 | 2026-11-08 |
| gmail | In the Gmail app, make a label for our emails and turn on notifications with a sound of their own for that label. | https://support.google.com/mail/answer/1075549 | 2026-11-08 |
| gmail | In your phone's Settings, open Do Not Disturb and add Gmail to the apps that can still alert you. | https://support.google.com/android/answer/9069335 | 2026-11-08 |
| outlook | In the Outlook app, turn on notifications and a sound for the account that gets our mail. | https://support.microsoft.com/en-us/outlook/how-can-i-turn-push-notifications-and-sounds-on-or-off | 2026-11-08 |
| outlook | Let the Outlook app through Do Not Disturb (Android) or Focus (iPhone). | https://support.google.com/android/answer/9069335 | 2026-11-08 |
| yahoo | Turn on alerts in the Yahoo Mail app and let it through Do Not Disturb or Focus. | https://support.google.com/android/answer/9069335 | 2026-11-08 |
| other | Turn on alerts for your mail app and let it through Do Not Disturb or Focus. | none | 2026-11-08 |
