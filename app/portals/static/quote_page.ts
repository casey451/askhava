// The quote page's live list (PICK-009, A11Y-033, D-492). While the job is live it fetches this
// same page every 30 seconds, adds each quote it has not shown yet at the end of the list, swaps a
// quote that became "no longer available", and writes one short line per batch into the status
// region, never one line per quote. Every word it writes comes from the page's own data
// attributes, so the words stay in the template. Without JavaScript the page works the same on a
// reload.

const POLL_MS = 30_000;

function byNumber(list: HTMLElement): Map<string, Element> {
  const found = new Map<string, Element>();
  for (const item of Array.from(list.children)) {
    const n = item.getAttribute("data-number");
    if (n !== null) {
      found.set(n, item);
    }
  }
  return found;
}

function merge(list: HTMLElement, fresh: Element): number {
  const shown = byNumber(list);
  let added = 0;
  for (const item of Array.from(fresh.children)) {
    const n = item.getAttribute("data-number");
    if (n === null) {
      continue;
    }
    const old = shown.get(n);
    if (old === undefined) {
      list.append(document.importNode(item, true));
      added += 1;
    } else if (old.getAttribute("data-available") !== item.getAttribute("data-available")) {
      old.replaceWith(document.importNode(item, true));
    }
  }
  return added;
}

function summary(status: HTMLElement, added: number): string {
  if (added === 1) {
    return status.dataset.one ?? "";
  }
  return (status.dataset.many ?? "").replace("{n}", String(added));
}

async function refresh(list: HTMLElement, status: HTMLElement): Promise<boolean> {
  const response = await fetch(window.location.href, {
    credentials: "same-origin",
    cache: "no-store",
    headers: { Accept: "text/html" },
  });
  if (!response.ok) {
    return false;
  }
  const page = new DOMParser().parseFromString(await response.text(), "text/html");
  const fresh = page.getElementById("quotes");
  if (fresh === null) {
    return false;
  }
  const added = merge(list, fresh);
  if (added > 0) {
    document.getElementById("no-quotes")?.remove();
    status.textContent = summary(status, added);
  }
  return fresh.getAttribute("data-live") === "true";
}

function start(): void {
  const list = document.getElementById("quotes");
  const status = document.getElementById("quote-status");
  if (list === null || status === null || list.dataset.live !== "true") {
    return;
  }
  const tick = (): void => {
    refresh(list, status).then(
      (live) => {
        if (live) {
          window.setTimeout(tick, POLL_MS);
        }
      },
      () => window.setTimeout(tick, POLL_MS),
    );
  };
  window.setTimeout(tick, POLL_MS);
}

start();

export {};
