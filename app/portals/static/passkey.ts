// The owner's passkey sign-in in the browser (T086; SEC-005, D-098, D-1919). Pressing the page's
// "Use a passkey" button POSTs the form's CSRF token to the button's data-route, which answers
// {"challenge": "<row ID>", "options": "<webauthn-rs JSON, base64url binary fields>"} (the page's
// GET stores no challenge). The script puts the row ID in the hidden "challenge" field, asks the
// browser for the passkey, writes the answer as JSON into the hidden "passkey" field and submits
// the form, so the password and the passkey go to the server together. Every word shown comes from the page; without
// JavaScript the password and code still work.

type Json = Record<string, unknown>;

function fromBase64Url(text: string): ArrayBuffer {
  const base64 = text.replace(/-/g, "+").replace(/_/g, "/");
  const padded = base64 + "=".repeat((4 - (base64.length % 4)) % 4);
  const bytes = Uint8Array.from(atob(padded), (c) => c.charCodeAt(0));
  return bytes.buffer;
}

function toBase64Url(buffer: ArrayBuffer | null): string | null {
  if (buffer === null) {
    return null;
  }
  const text = String.fromCharCode(...new Uint8Array(buffer));
  return btoa(text).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function requestOptions(raw: string): PublicKeyCredentialRequestOptions {
  const options = (JSON.parse(raw) as { publicKey: Json }).publicKey;
  const allow = (options.allowCredentials ?? []) as { id: string; type: "public-key" }[];
  return {
    ...options,
    challenge: fromBase64Url(options.challenge as string),
    allowCredentials: allow.map((c) => ({ type: c.type, id: fromBase64Url(c.id) })),
  };
}

function answerJson(credential: PublicKeyCredential): string {
  const response = credential.response as AuthenticatorAssertionResponse;
  return JSON.stringify({
    id: credential.id,
    rawId: toBase64Url(credential.rawId),
    type: credential.type,
    extensions: {},
    response: {
      authenticatorData: toBase64Url(response.authenticatorData),
      clientDataJSON: toBase64Url(response.clientDataJSON),
      signature: toBase64Url(response.signature),
      userHandle: toBase64Url(response.userHandle),
    },
  });
}

async function fetchOptions(form: HTMLFormElement, route: string): Promise<{ challenge: string; options: string }> {
  const csrf = new FormData(form).get("csrf");
  const reply = await fetch(route, {
    method: "POST",
    credentials: "same-origin",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({ csrf: typeof csrf === "string" ? csrf : "" }),
  });
  if (!reply.ok) {
    throw new Error("no passkey options");
  }
  return (await reply.json()) as { challenge: string; options: string };
}

async function signInWithPasskey(button: HTMLButtonElement): Promise<void> {
  const form = document.getElementById("owner-signin") as HTMLFormElement | null;
  const field = document.getElementById("passkey-answer") as HTMLInputElement | null;
  const challenge = document.getElementById("passkey-challenge") as HTMLInputElement | null;
  const status = document.getElementById("passkey-status");
  if (form === null || field === null || challenge === null || button.dataset.route === undefined) {
    return;
  }
  try {
    const issued = await fetchOptions(form, button.dataset.route);
    challenge.value = issued.challenge;
    const publicKey = requestOptions(issued.options);
    const credential = await navigator.credentials.get({ publicKey });
    if (credential instanceof PublicKeyCredential) {
      field.value = answerJson(credential);
      form.submit();
    }
  } catch {
    if (status !== null) {
      status.textContent = status.dataset.failed ?? "";
    }
  }
}

const button = document.getElementById("passkey-button");
if (button instanceof HTMLButtonElement && "credentials" in navigator) {
  button.addEventListener("click", () => {
    void signInWithPasskey(button);
  });
}
