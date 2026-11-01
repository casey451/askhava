// The quote page's live list (PICK-009, A11Y-033, D-492). While the job is live it fetches this
// same page every 30 seconds, adds each quote it has not shown yet at the end of the list, swaps a
// card that became "no longer available" or whose Pick form came or went, and writes one short
// line per batch into the status region (new quotes and quotes no longer available together),
// never one line per quote. Every word it writes comes from the page's own data attributes, so
// the words stay in the template. Without JavaScript the page works the same on a reload.

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

interface Batch {
  added: number;
  gone: number;
}

// A card is replaced when its quote became unavailable or its Pick form came or went
// (data-pickable), so a closed job loses its Pick buttons on the same fetch.
function changed(old: Element, item: Element): boolean {
  return ["data-available", "data-pickable"].some(
    (name) => old.getAttribute(name) !== item.getAttribute(name),
  );
}

function merge(list: HTMLElement, fresh: Element): Batch {
  const shown = byNumber(list);
  const batch: Batch = { added: 0, gone: 0 };
  for (const item of Array.from(fresh.children)) {
    const n = item.getAttribute("data-number");
    if (n === null) {
      continue;
    }
    const old = shown.get(n);
    if (old === undefined) {
      list.append(document.importNode(item, true));
      batch.added += 1;
    } else if (changed(old, item)) {
      const wasOpen = old.getAttribute("data-available") === "true";
      if (wasOpen && item.getAttribute("data-available") === "false") {
        batch.gone += 1;
      }
      old.replaceWith(document.importNode(item, true));
    }
  }
  return batch;
}

function words(one: string | undefined, many: string | undefined, count: number): string {
  if (count === 0) {
    return "";
  }
  if (count === 1) {
    return one ?? "";
  }
  return (many ?? "").replace("{n}", String(count));
}

// One status line per batch (A11Y-033): new quotes and quotes no longer available share it.
function summary(status: HTMLElement, batch: Batch): string {
  const d = status.dataset;
  return [words(d.one, d.many, batch.added), words(d.goneOne, d.goneMany, batch.gone)]
    .filter((part) => part !== "")
    .join(" ");
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
  // This merge runs on every fetch, including the last one that finds the job no longer live,
  // so the final state is shown before polling stops.
  const batch = merge(list, fresh);
  if (batch.added > 0) {
    document.getElementById("no-quotes")?.remove();
  }
  const line = summary(status, batch);
  if (line !== "") {
    status.textContent = line;
  }
  const live = fresh.getAttribute("data-live") === "true";
  list.dataset.live = String(live);
  return live;
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
