Draft prepared with AI research; not reviewed by a lawyer.
Version 0.2 (draft, submission package, D-5278) · November 21, 2026 · Not yet submitted

# Laftly texting campaign: the homeowner number

This is the submission package for the Twilio campaign on the Laftly number. Nothing here is submitted, signed or registered until the owner says go. The owner registers it by hand in the Twilio Console, in the order the account checklist gives. An approved campaign can't be edited, so the first registration has to be complete: both languages, all five samples, the opt-out and help texts below.

## 1. Before registering

Register only after all of these are true:

- laftly.com and the SMS terms show the Laftly LLC mailing address (a P.O. box or a registered private mailbox).
- The full site is live on laftly.com, including the request page, the privacy policy and the SMS terms.
- The owner has sent Twilio the written question in `docs/handoff/Twilio_question.md` and filed Twilio's written answer. If the answer is unclear, the same question goes to one other provider, and its written answer is filed too.
- The answer doesn't put the service under error 30951. If it does, don't register: see section 8.
- The pre-submit check in section 9 is done.

## 2. Campaign settings

- Brand: Laftly LLC.
- Brand type: Low-Volume Standard.
- Use case: Low Volume Mixed.
- If Twilio's written answer names a different brand type or use case, use the one it names and log the change.
- Tick "Messages will include embedded links": yes. Every link is a full laftly.com link.
- Tick "Messages will include phone numbers": yes. The who's-calling text includes the masked number the business calls from.
- Content: texts between Laftly and homeowners about their own request only. No marketing. No texts to businesses.
- Languages: English and Spanish (section 7).

## 3. Campaign description

> Laftly LLC runs Laftly, a quote-request service for homes in Lake Havasu City, AZ. A homeowner finds our number on a public page on laftly.com, next to our SMS disclosures, and texts us first. Our first reply is the full consent text: message frequency, rates, HELP and STOP. Only Laftly texts her, and only about her own request: questions about the job, confirmations, when quotes arrive, her pick and who will call her. Local businesses get the job without her name, phone, email or street address and post quotes on laftly.com. The business she picks pays Laftly a fixed introduction fee by job size: $25, $75 or $199. Some jobs are fee-free. Only then does that business see her first name and the job address. Her phone number and her SMS consent are never given to any business; calls go through a masked number. We send no marketing, and businesses never send messages on this campaign. Businesses get no texts on this number; they are notified by email. A person who texts asking to join as a business gets one reply with the sign-up link. Texts are in English. Once we turn Spanish on, people who write to us in Spanish get Spanish texts.

## 4. Who gets texts on this number

Only homeowners asking for quotes on a job at their own home. Businesses get their notices by email. The one exception is the single reply with the sign-up link to a person who texts asking to join as a business.

If business texting is ever approved, it gets its own campaign on its own number with its own description. Its samples are a job card (the new job notice), a pick notice and the STOP reply. It never runs on this number.

## 5. How people opt in

There is one way to opt in: the person's own first text to the Laftly number, sent after a public page on laftly.com that shows the number and the SMS disclosures. Nothing else signs anyone up for texts. A sign-in code sent by Twilio Verify is not an opt-in.

Message flow (the `message_flow` field):

> A person opts in by sending their own first text to the Laftly number. They find the number on a public page on laftly.com that shows it next to our SMS disclosures: automated texts about their own request, message frequency varies, message and data rates may apply, reply HELP for help and STOP to opt out, with links to laftly.com/sms-terms, laftly.com/terms and laftly.com/privacy. Texting us is optional, and nobody has to reply to agree. Our first reply is the consent text (sample 1).

End the field with the evidence link from the pre-submit check (section 9). The SMS terms are at laftly.com/sms-terms.

The privacy policy says: "No mobile information will be shared with third parties or affiliates for marketing or promotional purposes."

Opt-out message (Advanced Opt-Out), English. It is the exact CON-009 confirmation, and it covers the STOP reply, so the STOP reply is not a sample:

```text
Laftly: You're unsubscribed. You won't get more texts or automated calls from Laftly at this number. To use Laftly again, text START.
```

Opt-out message (Advanced Opt-Out), Spanish:

```text
Laftly: Se canceló su suscripción. No recibirá más textos ni llamadas automáticas de Laftly en este número. Para volver a usar Laftly, envíe START.
```

Help: the provider's own HELP auto-reply stays off. The app sends the full help reply in every language, unchanged. The `help_message` field below is word for word from the live English help reply, made only of its whole sentences, and is 320 characters or fewer. If Twilio ever refuses the registration because this field and the live reply differ, the choice is looked at again.

```text
Laftly: Tell us what's wrong in one text. The Laftly owner gets HELP messages right away and usually replies within 2 days, or within 24 hours if your job is urgent. This number is text-only. If anyone is in danger, call 911. Help: help@laftly.com or laftly.com/help Reply STOP to opt out.
```

## 6. Sample messages

Each sample below is a real template, copied word for word. Words in braces are filled in for each text: {job} is the request number, {link} the private quote page link, {quoted} the quote number, {business} the business name and {number} the masked number the business calls from. A docs test fails if a sample differs from its template. The reply to a person who asks to join as a business stays in the product and is named in the description, not sent as a sample.

Sample 1, the consent text. Template `consent`:

```text
Laftly: Got it. Next, a few quick questions. Laftly is not a contractor; you hire the business you pick directly. Automated texts about your request. Msg frequency varies. Msg & data rates may apply. Reply STOP to opt out, HELP for help. Terms: laftly.com/terms Privacy: laftly.com/privacy
```

Sample 2, your first quote is in. Template `first_quote`:

```text
Laftly: Your first quote for request {job} is in, and more may come. Free for you. The business you pick pays Laftly a set introduction fee. Can't open links? Reply LIST to get your quotes by text. See them here: {link} Terms (18+): laftly.com/terms
```

Sample 3, the pick confirmation. Template `text_pick_done`:

```text
Laftly: Done. You picked quote {quoted} for request {job}. We'll text you when the business accepts. You can still pick another quote. Reply LIST to see them.
```

Sample 4, who's calling, with the masked number. Template `whos_calling`:

```text
Laftly: {business} will call you about request {job} from {number}. If you miss it, call them back on this number. It works for 30 days. Ask for the price and any changes in writing before work starts. Don't pay in full up front.
```

Sample 5, the consent text in Spanish. Spanish template `consent`:

```text
Laftly: Recibido. Ahora, unas preguntas. Laftly no es contratista; usted contrata al negocio que elija. Textos automatizados sobre su solicitud; frecuencia variable. Pueden aplicar cargos de msj y datos. PARAR para salir, AYUDA para ayuda. Términos: laftly.com/terms Privacidad: laftly.com/privacy
```

### Console copy

Paste these into the Console, in this order. Each filled value is in [brackets], and nothing is left in braces.

Console sample 1:

```text
Laftly: Got it. Next, a few quick questions. Laftly is not a contractor; you hire the business you pick directly. Automated texts about your request. Msg frequency varies. Msg & data rates may apply. Reply STOP to opt out, HELP for help. Terms: laftly.com/terms Privacy: laftly.com/privacy
```

Console sample 2:

```text
Laftly: Your first quote for request [request number] is in, and more may come. Free for you. The business you pick pays Laftly a set introduction fee. Can't open links? Reply LIST to get your quotes by text. See them here: https://laftly.com/q/[private code] Terms (18+): laftly.com/terms
```

Console sample 3:

```text
Laftly: Done. You picked quote [quote number] for request [request number]. We'll text you when the business accepts. You can still pick another quote. Reply LIST to see them.
```

Console sample 4:

```text
Laftly: [business name] will call you about request [request number] from [masked phone number]. If you miss it, call them back on this number. It works for 30 days. Ask for the price and any changes in writing before work starts. Don't pay in full up front.
```

Console sample 5:

```text
Laftly: Recibido. Ahora, unas preguntas. Laftly no es contratista; usted contrata al negocio que elija. Textos automatizados sobre su solicitud; frecuencia variable. Pueden aplicar cargos de msj y datos. PARAR para salir, AYUDA para ayuda. Términos: laftly.com/terms Privacidad: laftly.com/privacy
```

## 7. Spanish

The first registration declares English and Spanish, with the Spanish consent text as sample 5. Texts go out in English until the "Spanish conversations" setting is turned on; the registration doesn't change when it is.

Spanish review:

- Before launch, only the Spanish texts that stay on while Spanish conversations are off are checked. Two AI tools translate them back to English for that check.
- The paid professional review of every Spanish template comes before the Spanish conversations setting is turned on.

## 8. If the campaign is refused

Find the error code on the campaign page in the Console before changing anything. Never register a second campaign with softer wording for the same texts.

If the refusal is error 30951, or the written answer puts the service under it, there is no registration and no toll-free fallback, because toll-free has the same ban (30468). Don't resubmit. The service stays in web-and-email mode, and the choice goes back to the owner.

If the homeowner campaign is refused for any other reason, then only with the owner's go, the fallback is toll-free verification for the homeowner texts, with the same content, consent text and samples. The toll-free number is never used for business notices without the written approval business texting needs. It is never used to get around a suspension of a working number. Until it is verified, the service runs in web-and-email mode: homeowners get their updates on the web page and by email, not by text.

## 9. Pre-submit check

Do all of these right before submitting:

1. Check the description, the message flow and every sample against Twilio's refusal codes from December 9, 2026: 30977, 30984 (a placeholder left unfilled), 30988 (a chat widget named but not on the site), 30965, 30987, 30966 and 30978.
2. Add a public screenshot or evidence link of the page that shows the number and the SMS disclosures, as it will look at launch. No number is public before Gate C, and there is no exception to that.
3. Check that no unfilled placeholder is left: the Console copy has only [brackets], with nothing in braces or angle brackets.
4. Check that the `help_message` text is still 320 characters or fewer and still matches the live English help reply.

## 10. When the texts change

Any new kind of text on this number goes to homeowners only. If a new text changes what the campaign sends, update this description and the samples first, then the Console. An approved campaign can't be edited, so a change to the samples or the description means a new registration.
