# DNS records for laftly.com

What Casey sets at the DNS host (Cloudflare DNS) when the domain goes live (T116; WEB-016, SEC-053, SEC-054, OPS-045, D-492, D-4914). Claude never changes DNS or creates accounts; this file is the checklist Casey follows by hand. A value written `<from ...>` is shown by the named service when Casey adds the domain there; none is stored in this repository.

## Web hostnames: all DNS-only (SEC-053)

Every web hostname is DNS-only (the grey cloud, not proxied through Cloudflare), so browsers and vendors reach the host's Let's Encrypt certificate directly and no Cloudflare bot check can challenge a webhook. Proxying a hostname later needs a decision that records the host's Full-not-Strict TLS requirement.

| Type | Name | Value | Proxy status |
|---|---|---|---|
| CNAME | `laftly.com` (root, flattened by the DNS host) | `<from the host: the web service's target>` | DNS-only |
| CNAME | `www.laftly.com` | `<from the host: the web service's target>` | DNS-only |
| CNAME | `hooks.laftly.com` | `<from the host: the web service's target>` | DNS-only |

`hooks.laftly.com` is the one hostname every vendor webhook uses (Twilio, Stripe, the inbound email provider).

## Certificate authority (SEC-054)

| Type | Name | Value | Proxy status |
|---|---|---|---|
| CAA | `laftly.com` | `0 issue "letsencrypt.org"` | DNS-only |

`letsencrypt.org` is the only certificate authority allowed. Cloudflare's partner authorities are added only while a hostname is proxied, and only by a decision.

## HSTS (SEC-054)

The app's `Strict-Transport-Security` header (SEC-012) is the only HSTS source. Cloudflare's HSTS setting stays off. laftly.com is not submitted to the HSTS preload list before the pilot ends.

## Search Console (WEB-016, D-087)

Any existing `google-site-verification` TXT record on the domain is kept as it is when the other records are replaced; Casey copies it before changing anything. If the old domain had none, nothing is added unless a verification tag is used on the new site.

| Type | Name | Value | Proxy status |
|---|---|---|---|
| TXT | `laftly.com` | `google-site-verification=<the existing value, kept>` | DNS-only |

## Email (OPS-045, D-4914)

| Type | Name | Value | Purpose |
|---|---|---|---|
| MX, TXT (SPF, DKIM) | `notify.laftly.com` | `<from the email provider's domain page>` | Sending job cards and notices as `jobs@notify.laftly.com` |
| TXT | `_dmarc.laftly.com` | `<DMARC policy, set with the email provider's records>` | DMARC for the whole domain |
| MX | `laftly.com` | `<from the email provider's receiving page>` | All inbound mail, including `help@laftly.com`, delivered to the app's signed webhook (PRIV-026, SEC-025, OPS-036) |
| TXT (SPF) | `laftly.com` | `v=spf1 -all` (or the provider's value `<from the email provider's domain page>` if it asks) | The root domain sends nothing; `notify.laftly.com` is the only sender |
| TXT (DKIM) | `<selector from the email provider>._domainkey.laftly.com` | `<from the email provider's domain page>` | Signs mail if the provider ever sends from the root domain |

The decision D-4914 in `docs/decisions.md` records that help-address mail is received by the provider and reaches the app by webhook, no human mailbox holds it, and every service that sends as laftly.com.
