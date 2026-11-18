/// <reference types="node" />
// The accessibility statement page in a browser (T115; TD-WEB-08, A11Y-020, CALL-020; D-379,
// D-3155, D-4240).
//
// `/accessibility` shows docs/legal/accessibility-statement.md, the one copy of the statement: the
// main region's text holds every non-blank line of the document (a leading `#` run or `- `
// removed) and each line A11Y-020 requires, the help address's domain read from the brand file.
// While the number is text-only the page offers no phone line: no `tel:` link and no
// written number. This file holds no word list; the call-word scan is
// tests/docs/test_accessibility_statement.py's (D-4240). Fake data only.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { expect, open, test } from "./fixtures";

const REPO_ROOT = join(import.meta.dirname, "..", "..");
const DOMAIN = (JSON.parse(readFileSync(join(REPO_ROOT, "app/core/data/brand.json"), "utf8")) as { domain: string }).domain;

/** Whitespace runs as one space, trimmed. */
function squash(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

/** The document's non-blank lines, a leading `#` run or `- ` removed. */
const DOCUMENT_LINES = readFileSync(join(REPO_ROOT, "docs/legal/accessibility-statement.md"), "utf8")
  .split("\n")
  .map((line) => squash(line.replace(/^#+\s*/, "").replace(/^- /, "")))
  .filter((line) => line !== "");

/** A11Y-020's required lines (D-4240 rule 5). */
const REQUIRED = [
  "WCAG 2.2 Level AA",
  "Known limits",
  "by text",
  `by email at help@${DOMAIN}`,
  "the help page's form",
  "If you use a relay service, you can reach us by text or email.",
  "usually replies within 2 days",
];

test("TD-WEB-08: the statement page shows the whole statement and offers no phone line", async ({ page }) => {
  await open(page, "/accessibility", 1280);
  const main = squash((await page.locator("main").textContent()) ?? "");
  expect(DOCUMENT_LINES.length).toBeGreaterThan(0);
  for (const line of [...DOCUMENT_LINES, ...REQUIRED]) expect(main, line).toContain(line);
  await expect(page.locator('a[href^="tel:"]')).toHaveCount(0);
  expect(main).not.toMatch(/\d{3}[\s.-]\d{4}/);
});
