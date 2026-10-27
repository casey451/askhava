# Outbox file format

To tell or ask Casey something, the runner commits `outbox/<id>.md` to `claude/kit-state`. The `notify` workflow turns it into a GitHub issue (label `owner`, owner @-mentioned) and moves the file to `outbox/sent/`.

```
---
id: K3-q1
title: Question: ...
kind: question | approval | gate | blocked | info
---
<markdown body>
```

- `id`: letters, digits, `.`, `_`, `-`. Reusing an id adds a comment to the existing issue (reopening it if closed) instead of opening a new one. The issue carries the hidden marker `<!-- outbox-id: <id> -->`.
- `title`: the issue title (may contain colons).
- `kind`: one of `question`, `approval`, `gate`, `blocked`, `info`.
- A file that fails to parse stays in `outbox/` and fails the workflow run.

Casey's reply (a comment or closing the issue) comes back as `inbox/<issue>-<comment-id>.md` (or `inbox/<issue>-closed-<run_id>.md`) with a front-matter block (`issue`, `issue_title`, `outbox_id`, `event`, `author`, `url`, `timestamp`) followed by the comment text.
