# Brain: Deploying and operating a small Rust service on Railway with PostgreSQL

- **Purpose:** how Ask Hava's one image becomes a running `web` and `worker` on Railway, how it is configured, watched, backed up, restored and cut over at Gate C, and where the plan meets or misses what Railway, Cloudflare and the operations literature say.
- **Who reads it:** any session on the `ops` or `recovery` crates, the binaries' startup and shutdown, the Dockerfile (T019), `/healthz` and the health page (T106), the nightly export (T109), the runbook (T117), the DNS records file (T116), the account checklist (T122), the Railway test project (T135), the Railway restore drill (T139), the handover pack (T141) and Gate C (T145); requirements OPS-001 to OPS-047, SEC-001 to SEC-003, SEC-012, SEC-021, SEC-035, SEC-036, SEC-040, SEC-044, SEC-045, SEC-049, PRIV-016 to PRIV-018, PRIV-024, PRIV-029.
- **How to use it:** read the section for your task, run the checklist (section 14) before hand-in, and cite the principle tag (`O1` to `O26`) and the source tag (`[H#]`, section 15) when you rely on a rule.
- **Last verified:** 2026-10-29 (every `[H#]` page fetched that day).
- **It never overrides `docs/spec/` or `docs/architecture.md`.** If this file and the spec disagree, build the spec and report the disagreement in your task report. `[inferred]` = reasoning no fetched source states; **verify with host** = a host claim to check in the dashboard before relying on it. Database internals (roles, pools, timeouts, PITR detail) are in `postgres-operations.md` (cited as PG-P#); binary shutdown and request IDs are in `rust-production.md` (cited as R#). This file does not repeat them.

Task-number note: the brief that commissioned this file named T119 for deploy and T137 for the restore drill; in `kit/tasks.json` those are the J4 journey and the real-AI test. The deploy tasks are T019, T135 and T139, used here.

## 1. Who this draws on

- **Railway's own documentation** (it is the host; its docs define what a deploy, variable, backup and restart are on its platform) [H1] to [H15]. Railway community answers by staff and moderators [H16] are used only where the docs are silent, and are marked.
- **The Twelve-Factor App** (Heroku's statement of how a hosted app separates config, build and run) [H17].
- **Google's SRE book and workbook** (the standard text on monitoring and paging) [H18].
- **PostgreSQL documentation** on logical dumps and continuous archiving [H19], with the PostgreSQL brain.
- **Cloudflare, Let's Encrypt and the HSTS preload list** on DNS, proxying, TLS modes and certificate authority records [H20] [H21].
- **Stripe and Twilio** on webhook source addresses and signatures [H22] [H23].
- **OWASP** ASVS 5.0 V13 (configuration), the Secrets Management, Logging and Docker Security cheat sheets [H24].
- **Sentry, tracing-subscriber and Tokio** maintainers' docs [H25] [H26]; **Docker, cargo-chef and distroless** [H27]; **UptimeRobot** [H28]; **Cloudflare R2 and Backblaze B2** key models [H29].

## 2. The two projects (SEC-003)

**O1. Test and live share nothing, and Railway's own copy features respect that.** SEC-003 and Arch 9 fix two Railway projects, each with `web`, `worker`, PostgreSQL 16, its own keys, bucket, numbers and webhook endpoints. Railway's private network is per environment: "Each environment has its own isolated network", and service-to-service traffic "uses Wireguard encryption" [H4]. Sealed variables "are not copied when duplicating an environment" or a service [H3], so duplicating the test project to start live carries no test secret across, but every live secret must be entered fresh. Backup restores "can be limited to the same project and environment" [H12], so live data never lands in test through a restore. Twelve-Factor's config rule is the reason: config is "everything that is likely to vary between deploys" and env vars "are never grouped together as 'environments'" [H17]. Serves SEC-003, SEC-036, Arch 9.

**O2. One region, the nearest one.** Railway's US regions are US West (California, `us-west2`) and US East (Virginia) [H13]. Both projects run in `us-west2`, the closer to Lake Havasu City [inferred from the list]; the database sits in the same region as the services so every query stays on the private network [inferred].

## 3. What a deploy is, end to end

**O3. Build, release, run are separate stages, and a release is immutable.** "The twelve-factor app uses strict separation between the build, release, and run stages"; "Every release should always have a unique release ID" [H17]. For Ask Hava the release ID is the commit SHA in the OPS-043 release record.

**O4. The Railway sequence.** (1) Build the Dockerfile (builder `DOCKERFILE`) [H8]. (2) Pre-deploy command, if set: it runs "between building and deploying", "in a separate container", "within your private network" with "your application's environment variables", volumes not mounted; "If your command fails, it will not be retried and the deployment will not proceed" [H2]. (3) Start the new deployment and poll the healthcheck path until a 2xx; the default timeout is 300 seconds, after which "the deploy will be marked as failed"; Railway checks from the hostname `healthcheck.railway.app` [H1]. (4) Rollover: the old deployment "remains active" for the overlap time, then gets SIGTERM and, after the draining time, SIGKILL; both are settings (`RAILWAY_DEPLOYMENT_OVERLAP_SECONDS`, `RAILWAY_DEPLOYMENT_DRAINING_SECONDS`) [H7]. (5) Rollback returns to an earlier successful deployment within the plan's retention: "Both the Docker image and custom variables are restored during the rollback process" [H6]. This answers R14's "verify with the host": Railway's contract is SIGTERM, then SIGKILL after the draining seconds.

**O5. The old release is still serving when the new migration runs.** Steps 2 to 4 mean the previous `web` and `worker` keep running against the migrated schema until the overlap ends. So every migration must leave the previous release working: expand now, contract in a later release [inferred from H2, H7; PG-P2 gives the lock-safe forms]. Twelve-Factor's admin-process rule says the migration runs "against a release, using the same codebase and config" [H17], which `server migrate` from the same image does.

**O6. The migration credential follows the variables.** The pre-deploy container gets the service's own variables [H2], so a migration-role URL used there is also present in the running `web` process. D-480 left "database TLS and migration credentials" open (P-1). OWASP: "Secrets must have the minimum privileges assigned to them" [H24]. See proposal 3.

**O7. One image, two services, two start commands.** Arch 2 builds `server` and `worker` from one Dockerfile; on Railway each is its own service with its own start command and settings [H10]. Each service runs its own build of the same SHA [inferred: Railway builds per service; a byte-identical image is not promised]. The worker has no HTTP port, so it has no healthcheck path; Railway makes it active as soon as it starts [inferred from H1]. Draining seconds for the worker must exceed its per-handler timeout so R14's drain completes [inferred].

**O8. Autodeploy differs by project.** Test: GitHub autodeploy from `main` with "Wait for CI", which holds a deployment "in a `WAITING` state until every GitHub Actions check suite on the commit has finished" and skips it if one fails [H9]. Live: autodeploy disabled ("Click Disable to stop deploying automatically") [H9]; Railway's documented manual path deploys "the latest commit on the connected branch" [H9], not an arbitrary SHA, so OPS-043's "deploys only a commit SHA named in a release record" needs a branch whose tip is that SHA (proposal 4).

**O9. Railway's config file is going away.** Config as Code (`railway.toml`/`railway.json`) "is deprecated"; "Existing Config as Code files stop being read on 2026-12-01 (hard cutoff)" and new services cannot opt in; the replacement, Infrastructure as Code, is a TypeScript file `.railway/railway.ts` (Python and Go in beta) that also manages variables [H8]. T135 owns `railway.toml`; that file would be ignored by the time live starts. TypeScript is allowed only for browser extras (D-453). See proposal 1.

## 4. The image (SEC-021, SEC-035, SEC-045)

**O10. Multi-stage, dependencies cached, nothing but the binaries shipped.** Docker: "Multi-stage builds let you reduce the size of your final image" [H27]. cargo-chef's `prepare` writes `recipe.json` from the manifests and `cook` compiles the dependencies in their own layer, which stays cached "as long as your dependencies do not change" [H27]; it is a build tool and needs a `docs/decisions.md` entry under the new-library rule [inferred from BUILD_BRIEF.md:122]. The runtime stage copies only `server` and `worker`, the static assets and the templates the binaries need. Builds use `SQLX_OFFLINE=true` (Arch 2) and never enable `test-control` (SEC-035).

**O11. Base image pinned by digest, with the cost of pinning paid on purpose.** A digest in `FROM image@sha256:...` gives the same image every time; Docker warns it "does therefore not pull updated versions of an image, which may include security updates" [H27]. So a pinned digest needs something that proposes new ones (proposal 13). Arch 11 already picks a Debian slim runtime with `libssl3` and CA certificates because webauthn-rs links OpenSSL; distroless `cc-debian13` exists with `nonroot` tags [H27] but its library contents are not stated on its page, so keep Debian slim unless a trial proves otherwise [inferred].

**O12. Non-root, no secrets, no repository metadata.** OWASP: "Configuring the container to use an unprivileged user is the best way to prevent privilege escalation attacks"; "Ensure a `USER` directive is specified" [H24]. ASVS 13.4.1: deploy "without any source control metadata, including the .git" folder [H24], so `.dockerignore` excludes `.git`, `.env*`, `target/`, `twin/`, `stand_ins/` and `tests/` [inferred list]. No secret in a build argument or `ENV` line; keys arrive only as Railway variables at run time (SEC-001).

## 5. Variables and secrets (SEC-001, SEC-002, SEC-036, OPS-022)

**O13. Sealed variables are write-only, which is the point and the risk.** A sealed value "is provided to builds and deployments but is never visible in the UI nor can it be retrieved via the API"; it "cannot be un-sealed" and is not provided to `railway variables` or `railway run` [H3]. Seal every live key, `VAULT_KEY`, `VAULT_INDEX_KEY`, `ID_SECRET`, the export key and the bootstrap token. Because Railway will never show them again, the PRIV-024 paper escrow is the only recovery copy of `VAULT_KEY`; losing it loses the vault [inferred from H3].

**O14. Claude never holds a hosting key, so Claude never sees a live value.** OPS-022 and the deny rules forbid Railway CLI deploys. The threat model's R11 records the residual: Railway variables, not a key vault, are the secret store (ASVS 13.3.1 asks for "a secrets management solution, such as a key vault") [H24]. OWASP notes env vars "may be included in logs or system dumps" [H24], which is why R6's `secrecy` wrapping and PRIV-016 matter more here.

**O15. Verify a variable without printing it.** Startup validation (SEC-002, SEC-036) refuses to start on a missing, malformed or wrong-environment value, and checks vendor account identifiers (Twilio Account SID, Stripe account ID, sending domain, bucket name), which are not secrets [Arch 5]. A deploy that passes `/healthz` therefore proves every variable parsed. Never log a value, its length or a hash of it; OWASP: secrets should "Never be logged" [H24]. Proposal 6 shows the result to Casey in the owner portal.

**O16. Rollback brings old variables back.** "Both the Docker image and custom variables are restored during the rollback process" [H6]. After rotating a leaked key, rolling back to a deployment from before the rotation would reinstate the leaked key [inferred from H6; whether sealed values are included is not stated: **verify with host**]. The runbook says: after a key rotation, fix forward; never roll back past the rotation.

## 6. Health endpoint, restarts and the health page (OPS-007, OPS-008, OPS-033)

**O17. Railway's healthcheck is a deploy gate, not a monitor.** The endpoint "is only called at the start of the deployment"; "Railway does not monitor the healthcheck endpoint after the deployment has gone live" [H1]. Restarts happen when a service "stops", for example when it "exits with a non-zero exit code"; the default policy is On Failure with at most 10 restarts; paid plans may set any policy and count [H5]. Healthchecks are not a restart trigger [H5]. So OPS-033's "the app host is set to restart the app automatically when its health check fails" is not something Railway does. A process that hangs without exiting stays up until Casey presses Restart (proposal 2).

**O18. `/healthz` says ok or not, and nothing else.** UptimeRobot's free plan checks every 5 minutes and allows commercial use [H28], which matches OPS-004 (two failed 5-minute checks) and OPS-029 (push after 15 minutes). ASVS 13.4.5: monitoring endpoints are "not exposed unless explicitly intended" [H24]. TD-OPS-02 wants `/healthz` to report the database and the worker; the safe form is a 200 or 503 from a database round trip and the freshness of a worker heartbeat row, with no version, host name, count or error text in the body [inferred]. It must accept the Host `healthcheck.railway.app` or Railway's deploy check fails "with status 400" [H1]. The detailed figures belong on the signed-in OPS-008 page.

## 7. Alerting (OPS-001, OPS-004, OPS-023, OPS-029, OPS-032)

**O19. Page on symptoms, keep pages few.** The SRE book: monitoring answers "what's broken, and why?"; "Every page should be actionable"; a page is for a condition that is "urgent, actionable, and actively or imminently user-visible"; "When pages occur too frequently, employees second-guess, skim, or even ignore incoming alerts"; the rules that catch real incidents "should be as simple, predictable, and reliable as possible" [H18]. The plan fits this: OPS-029 pushes only user-visible or money symptoms (site down 15 minutes, a suspended number, a ledger difference that persists, critical safety), everything else waits for the OPS-032 batch or the digest, and OPS-023 auto-acknowledges when the symptom clears (the workbook's "reset time") [H18]. Safe mode (OPS-001) is a slow, symptom-driven brake, not a page.

**O20. Low traffic needs count windows and synthetic checks.** The workbook's options for low-traffic services include "Synthesizing traffic" [H18]. OPS-004's "last 50 / last 200 texts" are count windows, which suit a service with few events, and UptimeRobot is the synthetic traffic [inferred mapping]. Of the four golden signals (latency, traffic, errors, saturation) [H18], the plan alerts on errors and on traffic only through delivery rates; saturation of the database volume has no alert (proposal 9).

**O21. Security signals are cause-based and must earn their pushes.** SEC-047 (vault read volume) and SEC-048 (security-event thresholds) push High alerts that can start safe mode. They are causes, not symptoms, so their thresholds should be tuned on the test project's twin traffic before Gate C to keep precision high [inferred from H18's precision and recall].

## 8. Logs and error reports (PRIV-016, PRIV-017, OPS-011, SEC-048)

**O22. One JSON line per event on stdout, in the shape Railway reads.** Twelve-Factor: each process writes its event stream "unbuffered, to `stdout`" [H17]. Railway parses JSON only when "The entire JSON log must be emitted on a single line", reads `message` and `level` fields, and keeps logs 30 days on Pro, 7 on Hobby; it drops lines beyond 500 per second per replica [H14]. tracing-subscriber's JSON format nests event fields under `fields` unless `flatten_event(true)` puts them "into the root object" [H26]. Request IDs come from R15. Railway's 30-day log store is a copy of the request log outside the app's control, so it may hold only what PRIV-016 allows [inferred].

**O23. What never enters a log or Sentry.** OWASP's exclusion list includes "Session identification values", "Access tokens", "Sensitive personal data", "Database connection strings" and "Encryption keys" [H24]: for Ask Hava, no phone, name, address, email, link token, Verify code, cookie or `DATABASE_URL`. Sentry's SDK "purposefully does not send PII" unless `send_default_pii` is on; `before_send` should "scrub any data before it is sent"; server-side scrubbing in the Sentry UI is a second layer that applies "immediately for new events" [H25]. Set both (PRIV-017).

## 9. Backups and restores (OPS-012 to OPS-019, PRIV-018, PRIV-029)

**O24. Railway's backups are real but live inside Railway.** Volume backups: daily kept 6 days, weekly 27 days, monthly 89 days; a restore lands on a new volume and is staged for a Deploy press; "wiping a volume deletes all of its backups" [H12]. PITR: about 4 weeks for Postgres, "not retroactively" from enabling, restoring to "a new sibling service", billed as bucket storage and egress [H12]; plan availability is not stated (**verify with host**, OPS-012). The template "deploys a single node"; an HA option with automatic failover exists [H12] and is not in the plan. All of this fails with the account or project, which is why OPS-013's nightly encrypted export to separate storage stays (D-094). PG-P14 has the rest.

**O25. A logical dump does not carry roles.** pg_dump "does not dump information about roles"; before restoring, "all the users who own objects or were granted permissions ... must already exist"; `--single-transaction` makes a restore "either fully completed or fully rolled back" [H19]. PITR, by contrast, restores "an entire database cluster, not a subset" [H19]. So the drill (PG section 3) runs the role bootstrap before `pg_restore` (proposal 7).

**O26. Separate storage must be separately locked.** PRIV-029 asks for a write-only, prefix-scoped token and object lock. Cloudflare R2 (D-094's default) documents token levels "Object Read & Write" and "Object Read only", none write-only, and bucket-level scoping only; it offers bucket locks that "prevent the deletion and overwriting of objects ... for a specified period" [H29]. Backblaze B2 keys can hold `writeFiles` without `readFiles` or `deleteFiles` and can be limited to one bucket and a file-name prefix [H29]. R2 storage is $0.015 per GB-month with 10 GB free and no egress charge [H29]. See proposal 8.

The off-database stream (OPS-010, OPS-011, T017) goes to the same storage under the same lock; it, Twilio's and Stripe's records are the replay inputs (OPS-015). The drill proves a restore only when it ends with the app switched to the restored database and `/healthz` green (OPS-014).

## 10. DNS, Cloudflare, TLS and webhooks (SEC-012, SEC-040, SEC-049, OPS-045)

- **Railway's certificate.** Railway "will automatically generate and apply a Let's Encrypt certificate", 90 days, renewed at 30 days left, issued "within an hour" of DNS; custom domains need "both the `CNAME` record and the `TXT` record" from the dashboard [H15].
- **Proxying through Cloudflare.** With the orange cloud, Railway says "you MUST set your SSL/TLS settings to **Full** -- Full (Strict) **will not work as intended**", and Cloudflare-to-Railway traffic then uses the `*.up.railway.app` certificate [H15]. Cloudflare recommends Full (strict), which needs an origin certificate matching the hostname [H20]. Full checks less. Gray cloud (DNS-only) returns the origin address and Cloudflare "cannot optimize, cache, or protect" the traffic [H20]. That is the trade.
- **Webhooks behind the proxy.** Bot Fight Mode "may challenge API or mobile app traffic" and "You cannot bypass or skip Bot Fight Mode using WAF custom rules or Page Rules" [H20]; a challenged Stripe or Twilio callback is a lost event. Twilio signs "the full URL with the scheme, port, query string" it requested, so any rewrite (even a trailing slash) breaks the check [H23].
- **Client IP.** Cloudflare's `CF-Connecting-IP` is sent only "from Cloudflare's edge to your origin" [H20]. Railway documents no client-IP header; a moderator says `X-Real-IP` and `X-Forwarded-For` "are all overwritten at Railway's edge" and "There are no documented ranges for Railway's proxies"; staff answers in another thread contradict each other [H16]. SEC-040's "the host's proxy hop" therefore has no documented address range: **verify with host** by measurement at T135.
- **Stripe's addresses.** Stripe publishes 15 webhook source IPs, a JSON copy at `https://stripe.com/files/ips/ips_webhooks.json`, and gives "seven days' notice" of changes on its API announce list [H22]. SEC-049's "refreshed from Stripe's list at startup" adds a startup dependency on stripe.com (proposal 4).
- **HSTS and CAA.** SEC-012's `includeSubDomains` makes any subdomain without HTTPS "inaccessible"; preload "can make a website without HTTPS completely inaccessible" [H20]; preload needs `max-age` of at least 31536000, `includeSubDomains` and `preload`, and "cannot easily be undone" [H21]. CAA limits which CAs may issue; Let's Encrypt's value is `letsencrypt.org` [H21]; if any CAA record exists and Universal SSL is on, Cloudflare adds its own CAs' records, and CAA on a CNAME target "take[s] precedence" [H20].
- **Stage 0.5.** No document fixes where the static pages of WEB-017 are served before Gate C; the live Railway project may not be used until then (BUILD_BRIEF.md:68). T116 needs that decision [inferred gap].

## 11. Cost and capacity

**Prices** [H11]: Pro $20 a month including $20 of usage; RAM $10 per GB-month, vCPU $20 per vCPU-month, volume $0.15 per GB-month, egress $0.05 per GB; Pro allows 42 replicas per service. **Estimate per project** [inferred]: `web` 0.25 GB and 0.05 vCPU ($3.50), `worker` 0.15 GB and 0.02 vCPU ($1.90), PostgreSQL 0.5 GB and 0.05 vCPU ($6.00), 2 GB volume ($0.30), a few GB of egress ($0.25): about $12 a month, so live plus a mostly idle test project should sit near the $20 included usage, roughly $20 to $35 a month in total, plus R2 or B2 pennies and UptimeRobot free. **Spikes** [inferred]: a memory leak (billed per GB-minute), replicas left raised, the database's public TCP proxy ("you will be billed for Network Egress", [H12]), PITR bucket growth, a test project left at live size. **Limits** [H11]: a usage alert emails at a set amount; a hard limit (minimum $10) takes "all your workloads ... offline" until it is raised. A hard limit is a self-inflicted outage for a service that holds people's jobs (proposal 10).

**Capacity** [inferred from Arch 5]: one `web` and one `worker` replica at launch. Nothing in the queue assumes one worker: claims use `SKIP LOCKED` with leases, nightly jobs use date-derived dedupe keys, rate-limit counters and sessions live in the database (Arch 5). Per-process state is the `ReleaseMint` (once per process, by design) and any cached list (for example Stripe's IPs). With replicas Railway spreads traffic "randomly" in one region [H13]. Scale `web` when p95 latency or CPU saturates; scale `worker` when task lateness (oldest due `run_at`) grows; recompute the PG-P8 connection budget, counting deploy overlap, before either.

## 12. Gate C cutover order (OPS-042 to OPS-044, T145)

1. Casey completes OPS-042 (hardware keys, registrar lock, recovery, escrow) and OPS-044 (Stripe public details).
2. Casey creates the live project in `us-west2`: autodeploy off, restart On Failure, PITR enabled and its date recorded (O24), backup schedules on, usage alert set, no public database proxy.
3. Casey enters every live variable, sealed; the bootstrap token last (SEC-044).
4. Casey points the live service at the signed release (OPS-043) and presses Deploy: migrations, bootstrap, `/healthz` green on the `*.up.railway.app` domain. Nothing public yet.
5. DNS (T116 file): CNAME and TXT for the app hostnames and the webhook hostname, CAA, keeping Search Console and mail records; wait for the certificate [H15].
6. Vendor endpoints switched to live: Stripe webhook endpoint and secret, Twilio number webhooks, Resend inbound and events; UptimeRobot monitor and Sentry live DSN.
7. Real-phone rehearsal on Casey's phone (BUILD_BRIEF.md:203); the texting number appears on the site only now (WEB-017).
8. First live charge, only on Casey's go (BUILD_BRIEF.md:552), then the nightly reconcile is checked the next morning.

## 13. Incident playbook (feeds T117's runbook; OPS-020, OPS-021)

| Symptom | First move | Then |
|---|---|---|
| UptimeRobot push: site down | Railway dashboard: is the deployment Crashed? Press Restart [H6] | If the last deploy caused it, Rollback, unless a key was rotated since (O16) |
| Deploy failed at pre-deploy or healthcheck | Nothing is live-changed; the old deployment still serves [H1][H2] | Read the build and deploy logs; fix forward in a new release |
| Process up but hung (`/healthz` 503, no crash) | Press Restart (Railway will not, O17) | Look for a stuck lock (PG failure-mode table) |
| Database volume near full | Grow the volume; stop nonessential nightly jobs | Find the growth (logs table, queue rows) |
| Usage alert email | Check replicas, memory graph, TCP proxy | Never set a hard limit as the fix |
| Webhook errors for 30 minutes (OPS-004) | Vendor status page; Stripe dashboard delivery attempts | Stripe retries for 3 days; events are deduped (SEC-017) |
| Secret exposed | OPS-021: pause texting and charging, keep logs (export the Railway log lines now; 30-day retention [H14]) | Rotate in the vendor, re-enter sealed variable, redeploy, never roll back past it; incident plan section 3 |
| Data lost or corrupt | Pause charging; restore to a new service (PITR or export) | OPS-014 switch, OPS-015 to OPS-019 replay |

## 14. Requirement map, proposed requirements and review checklist

**Right already:** separate projects and live autodeploy off (SEC-003, OPS-022); Claude holds no hosting key (OPS-022, threat E5); a signed release and independent review before live (OPS-043); startup validation of every setting and vendor account (SEC-002, SEC-036); image pinned by digest, Actions by SHA (SEC-021, SEC-045); test-control compiled out (SEC-035); export to separate storage with a monthly drill that ends in a switch (OPS-013, OPS-014); off-database stream for replay (OPS-010, OPS-011); symptom pushes with automatic reset and a slow safe-mode brake (OPS-001, OPS-023, OPS-029); request IDs and no phone numbers in logs, Sentry PII off (PRIV-016, PRIV-017); escrow of every key (PRIV-024).

**Proposed** (need a spec change logged in `docs/decisions.md`; a task may not add them on its own):

1. **No `railway.toml`** (T135, OPS-022, SEC-003): "The test and live service settings (builder, start commands, migration step, healthcheck path and timeout, restart policy, overlap and draining seconds, region, replicas, autodeploy and Wait for CI) are a table in `docs/deploy.md` that Casey enters in the dashboard and confirms; T135 owns no Railway config file, and adopting Railway Infrastructure as Code needs its own decision, since its file is TypeScript (D-453) and Config as Code stops being read on 2026-12-01." [H8]
2. **Self-exit on a hang** (OPS-033, OPS-007): "`web` and `worker` each exit with a non-zero code when their own liveness check (database round trip, and for the worker a claim-loop tick) has failed for 3 minutes in a row (a setting), and both services use the On Failure restart policy with at least 10 retries; OPS-033's 'restart when its health check fails' is reworded to this." [H1, H5]
3. **Migration credential only where migrations run** (D-480 P-1, SEC-044, Arch 9): "The migration-role database URL and the bootstrap token are sealed variables on a separate `migrate` service only (start command `server migrate`, restart policy Never), never on `web` or `worker`; a release deploys `migrate` first and deploys `web` and `worker` only after it exits 0, and every migration leaves the previous release working." [H2, H24, O5]
4. **Release branch, webhook host and Stripe list** (OPS-043, SEC-049, SEC-040, SEC-016): "The live services deploy from a protected `release` branch that only Casey moves to the SHA in the signed release record; vendor webhooks use a DNS-only hostname (`hooks.askhava.com`); the Stripe source-IP check reads the client address from the header T135's measurement shows Railway overwrites, recorded in `docs/deploy.md`, against a copy of Stripe's `ips_webhooks.json` committed in the repository and compared weekly with the published list, a difference raising a Low alert, instead of a fetch at startup." [H9, H16, H20, H22]
5. **TLS mode** (SEC-012): "Every askhava.com web hostname is DNS-only at launch so browsers and vendors reach Railway's Let's Encrypt certificate directly; proxying a hostname later needs a decision that records Railway's Full-not-strict requirement." [H15, H20]
6. **Secrets readout without values** (SEC-002, SEC-036, OPS-008): "The owner health page lists each required secret as present, parsed and environment-matched, and the SEC-036 account identifiers, never a value, length or hash of a value." [H24]
7. **Restore with roles first** (OPS-013, OPS-014, T109, T139): "The nightly export is `pg_dump -Fc` of the app database; every restore runs the role bootstrap before `pg_restore --single-transaction`, and the drill fails if any restored object is owned by a role other than the migration role or if the app role holds `UPDATE` or `DELETE` on an append-only table." [H19, PG-P3, PG-P5]
8. **Storage that can be write-only** (PRIV-029, D-094): "The storage provider is chosen only if its documented key model gives a key that can write but not read or delete, limited to one bucket and prefix, with retention lock (Backblaze B2 meets this; Cloudflare R2's documented token levels do not), or PRIV-029 is amended by decision." [H29]
9. **Saturation alert** (OPS-004): "A daily worker check compares database size with the volume size (a setting) and raises a Low alert above 70% and a pushed High alert above 85%." [H18]
10. **Spend alerts, no hard stop** (OPS-042, OPS-004): "The Railway workspace holding the live project has a usage email alert to the alert address at twice the projected monthly bill and no hard limit, since a hard limit takes every workload offline; the Gate C checklist lists it." [H11]
11. **Log shape** (PRIV-016, OPS-011): "The JSON log layer writes one line per event with `message` and `level` at the top level (`flatten_event(true)`), and the privacy data map lists Railway's 30-day log store." [H14, H26]
12. **HSTS and CAA** (SEC-012, T116): "`docs/dns-records.md` holds CAA records allowing `letsencrypt.org` (and Cloudflare's partner CAs while any hostname is proxied); the app's header is the only HSTS source, Cloudflare's HSTS setting stays off, and askhava.com is not submitted for preload before the pilot ends." [H20, H21]
13. **Base-image updates** (SEC-021, SEC-045): "Dependabot watches the Dockerfile's base images as well as Actions, so a new digest opens a PR that runs the full checks." [H27]
14. **Stage 0.5 host** (WEB-017, T116): "A decision before T116 names where the Stage 0.5 static pages are served (not the live Railway project) and how DNS moves from them to the app at Gate C step 5." [inferred]

**Review checklist for deploy and operations tasks**

- [ ] Nothing touches the live project, a live key or a hosting token; settings changes are written for Casey to enter (OPS-022, BUILD_BRIEF.md:68).
- [ ] The image is multi-stage, base pinned by digest, runs as a non-root `USER`, has no `.git`, `.env*` or secret, and never enables `test-control` (O10 to O12, SEC-021, SEC-035).
- [ ] Every migration keeps the previous release working through the overlap (O5, PG-P2).
- [ ] `web` and `worker` stop on SIGTERM within the draining seconds; the worker releases leases (O4, R14).
- [ ] `/healthz` returns only ok or 503, answers the Railway healthcheck host, and leaks no version or detail (O18).
- [ ] Startup validation covers every new variable; no value, length or hash is logged (O15, SEC-002).
- [ ] Logs are single-line JSON with top-level `message` and `level`, a request ID, and no phone, name, token, code or connection string (O22, O23, PRIV-016).
- [ ] Sentry PII is off with a `before_send` scrubber (O23, PRIV-017).
- [ ] Backup and restore code restores roles first, lands in a new database, and ends with the switch and `/healthz` (O24, O25, OPS-014).
- [ ] Any new alert is a symptom or has a tuned threshold, says what to do, and has a reset rule (O19 to O21, OPS-023).
- [ ] Host claims used (plan features, limits, headers, IP ranges) cite an `[H#]` page or say **verify with host**.
- [ ] `docs/architecture.md` and `docs/decisions.md` updated in the same PR when a setting, service or vendor changes.

## 15. Sources and why they are authoritative

Railway pages are the host's own documentation; [H16] is community answers by Railway staff and moderators, not documentation.

- [H1] Healthchecks: https://docs.railway.com/deployments/healthchecks
- [H2] Pre-deploy command: https://docs.railway.com/guides/pre-deploy-command
- [H3] Variables (sealed, shared, reference): https://docs.railway.com/guides/variables
- [H4] Private networking: https://docs.railway.com/reference/private-networking
- [H5] Restart policy: https://docs.railway.com/deployments/restart-policy
- [H6] Deployment actions (rollback, restart): https://docs.railway.com/deployments/deployment-actions
- [H7] Deployment teardown (overlap, draining): https://docs.railway.com/deployments/deployment-teardown
- [H8] Config as Code (deprecated): https://docs.railway.com/config-as-code/reference ; Infrastructure as Code: https://docs.railway.com/infrastructure-as-code
- [H9] GitHub autodeploys and Wait for CI: https://docs.railway.com/deployments/github-autodeploys
- [H10] Services and sources: https://docs.railway.com/services
- [H11] Plans and prices: https://docs.railway.com/pricing/plans ; usage limits: https://docs.railway.com/reference/usage-limits
- [H12] Backups: https://docs.railway.com/volumes/backups ; PITR: https://docs.railway.com/volumes/point-in-time-recovery ; PostgreSQL: https://docs.railway.com/databases/postgresql
- [H13] Scaling: https://docs.railway.com/deployments/scaling ; regions: https://docs.railway.com/deployments/regions
- [H14] Logs: https://docs.railway.com/observability/logs
- [H15] Custom domains and Cloudflare: https://docs.railway.com/networking/domains/working-with-domains
- [H16] Client-IP headers (community): https://station.railway.com/questions/which-client-address-headers-does-the-ra-fb9e36d7 ; https://station.railway.com/questions/does-the-edge-strip-client-x-forwarded-f-613e5cb6
- [H17] The Twelve-Factor App: https://12factor.net/config ; https://12factor.net/build-release-run ; https://12factor.net/admin-processes ; https://12factor.net/disposability ; https://12factor.net/logs ; https://12factor.net/dev-prod-parity
- [H18] Google SRE book, Monitoring Distributed Systems: https://sre.google/sre-book/monitoring-distributed-systems/ ; SRE workbook, Alerting on SLOs: https://sre.google/workbook/alerting-on-slos/
- [H19] PostgreSQL 16, SQL dump: https://www.postgresql.org/docs/16/backup-dump.html ; continuous archiving: https://www.postgresql.org/docs/16/continuous-archiving.html
- [H20] Cloudflare: Full (strict): https://developers.cloudflare.com/ssl/origin-configuration/ssl-modes/full-strict/ ; proxy status: https://developers.cloudflare.com/dns/proxy-status/ ; HTTP headers: https://developers.cloudflare.com/fundamentals/reference/http-headers/ ; Bot Fight Mode: https://developers.cloudflare.com/bots/get-started/bot-fight-mode/ ; HSTS: https://developers.cloudflare.com/ssl/edge-certificates/additional-options/http-strict-transport-security/ ; CAA: https://developers.cloudflare.com/ssl/edge-certificates/caa-records/
- [H21] Let's Encrypt CAA: https://letsencrypt.org/docs/caa/ ; HSTS preload list: https://hstspreload.org/
- [H22] Stripe domains and IP addresses: https://docs.stripe.com/ips
- [H23] Twilio webhook security: https://www.twilio.com/docs/usage/security
- [H24] OWASP ASVS 5.0 V13: https://github.com/OWASP/ASVS/blob/master/5.0/en/0x22-V13-Configuration.md ; Secrets Management: https://cheatsheetseries.owasp.org/cheatsheets/Secrets_Management_Cheat_Sheet.html ; Logging: https://cheatsheetseries.owasp.org/cheatsheets/Logging_Cheat_Sheet.html ; Docker Security: https://cheatsheetseries.owasp.org/cheatsheets/Docker_Security_Cheat_Sheet.html
- [H25] Sentry Rust, sensitive data: https://docs.sentry.io/platforms/rust/data-management/sensitive-data/
- [H26] tracing-subscriber JSON format (0.3.23): https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Json.html ; Tokio graceful shutdown: https://tokio.rs/tokio/topics/shutdown
- [H27] Docker build best practices: https://docs.docker.com/build/building/best-practices/ ; pull by digest: https://docs.docker.com/reference/cli/docker/image/pull/ ; cargo-chef: https://github.com/LukeMathWalker/cargo-chef ; distroless: https://github.com/GoogleContainerTools/distroless
- [H28] UptimeRobot plans: https://uptimerobot.com/pricing/
- [H29] Cloudflare R2 pricing: https://developers.cloudflare.com/r2/pricing/ ; R2 API tokens: https://developers.cloudflare.com/r2/api/tokens/ ; R2 bucket locks: https://developers.cloudflare.com/r2/buckets/bucket-locks/ ; Backblaze B2 application keys: https://www.backblaze.com/docs/cloud-storage-application-keys

Not legal advice; prices and plan features change, so re-check [H11], [H12] and [H28] at the account checkpoint.
