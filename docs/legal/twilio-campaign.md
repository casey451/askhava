Draft prepared with AI research; not reviewed by a lawyer.
Version 0.1 (draft, not registered) · November 17, 2026 · Not yet submitted

# Laftly texting campaign: the homeowner number

This is the text for the Twilio campaign on the Laftly number. Nothing here is submitted, signed or registered until the owner says go. The owner registers it by hand in the Twilio Console, in the order the account checklist gives.

## 1. Before registering

Register only after all of these are true:

- laftly.com and the SMS terms show the Laftly LLC mailing address (a P.O. box or a registered private mailbox).
- The full site is live on laftly.com.
- The owner has asked Twilio in writing whether businesses may get texts, and filed the answer. Register this homeowner campaign whatever that answer is.

## 2. Campaign settings

- Brand: Laftly LLC.
- Use case: Customer Care, Low-Volume Standard. Don't ask for pre-clearance.
- Tick "Messages will include embedded links": yes. Every link is a full laftly.com link.
- Tick "Messages will include phone numbers": yes. The who's-calling text includes the masked number the business calls from.
- Content: texts between Laftly and homeowners about their own request only. No marketing. No texts to businesses.

## 3. Campaign description

> Laftly LLC is a local service in Lake Havasu City, AZ. People text us, or start on laftly.com, to ask for quotes on a home or local service they need. They opt in by texting us first, or by entering in the laftly.com chat a code we text to their mobile number; our first reply states message frequency, rates, HELP and STOP. We text them only about their own request: confirmations, when quotes arrive, the business they choose and who will call them. We never share, sell or transfer phone numbers or opt-in consent, and businesses never send messages on this campaign. Businesses get no texts on this number; they are notified by email. A person who texts asking to join as a business gets one reply with the sign-up link. Use case: Customer Care, Low-Volume Standard.

## 4. Who gets texts on this number

Only homeowners asking for quotes on a job at their own home. Businesses get their notices by email. The one exception is the single reply with the sign-up link to a person who texts asking to join as a business.

If business texting is ever approved, it gets its own campaign on its own number with its own description. Its samples are a job card (the new job notice), a pick notice and the STOP reply. It never runs on this number.

## 5. How people opt in

A person opts in by texting the Laftly number, or by entering in the laftly.com chat a code we text to their mobile number. They agree only to automated texts about their own request. Our first reply is the consent text (sample 1). It gives the message frequency, rates, HELP, STOP and the links to the terms and the privacy policy. The SMS terms are at laftly.com/sms-terms.

The privacy policy says: "No mobile information will be shared with third parties or affiliates for marketing or promotional purposes."

## 6. Sample messages

Each sample below is a real template, copied word for word. Words in braces are filled in for each text: {job} is the request number, {link} the private quote page link, {quoted} the quote number, {business} the business name and {number} the masked number the business calls from. A docs test fails if a sample differs from its template.

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

Sample 4, the STOP reply. Template `opt_out_confirmation`:

```text
Laftly: You're unsubscribed. You won't get more texts or automated calls from Laftly at this number. To use Laftly again, text START.
```

Sample 5, who's calling, with the masked number. Template `whos_calling`:

```text
Laftly: {business} will call you about request {job} from {number}. If you miss it, call them back on this number. It works for 30 days. Ask for the price and any changes in writing before work starts. Don't pay in full up front.
```

Sample 6, the one reply to a person who asks to join as a business. Template `business_signup_reply`:

```text
Laftly: This number is for homeowners asking for quotes. To get job requests, sign up at laftly.com/b/signup
Reply STOP to opt out, HELP for help.
```

## 7. Spanish

Texts go out in English until the "Spanish conversations" setting is turned on. Before turning it on:

1. Add the sentence "Texts are in English or Spanish." to the campaign description in the Console.
2. Add the Spanish sample below as a sample message.
3. Log the change.

If Twilio asks for the Spanish sample again later, update it in the Console the same way and log that too.

Spanish sample, the consent text. Spanish template `consent`:

```text
Laftly: Recibido. Ahora, unas preguntas. Laftly no es contratista; usted contrata al negocio que elija. Textos automatizados sobre su solicitud; frecuencia variable. Pueden aplicar cargos de msj y datos. PARAR para salir, AYUDA para ayuda. Términos: laftly.com/terms Privacidad: laftly.com/privacy
```

Spanish review:

- Before launch, only the Spanish texts that stay on while Spanish conversations are off are checked. Two AI tools translate them back to English for that check.
- The paid professional review of every Spanish template comes before the Spanish conversations setting is turned on.

## 8. If the campaign is refused

Find the error code on the campaign page in the Console before changing anything. Never register a second campaign with softer wording for the same texts.

If the homeowner campaign is refused, then only with the owner's go, the fallback is toll-free verification for the homeowner texts, with the same content, consent text and samples. The toll-free number is never used for business notices without the written approval business texting needs. It is never used to get around a suspension of a working number. Until it is verified, the service runs in web-and-email mode: homeowners get their updates on the web page and by email, not by text.

## 9. When the texts change

Any new kind of text on this number goes to homeowners only. If a new text changes what the campaign sends, update this description and the samples first, then the Console.
