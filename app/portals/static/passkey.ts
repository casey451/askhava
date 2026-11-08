// The owner's passkey sign-in in the browser (T086; SEC-005, D-098, D-1919). The page's
// "Use a passkey" button carries the server's WebAuthn request options (webauthn-rs JSON, with
// base64url binary fields) in data-options. Pressing it asks the browser for the passkey, writes
// the answer as JSON into the form's hidden "passkey" field and submits the form, so the password
// and the passkey go to the server together. Every word shown comes from the page; without
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

async function signInWithPasskey(button: HTMLButtonElement): Promise<void> {
  const form = document.getElementById("owner-signin") as HTMLFormElement | null;
  const field = document.getElementById("passkey-answer") as HTMLInputElement | null;
  const status = document.getElementById("passkey-status");
  if (form === null || field === null || button.dataset.options === undefined) {
    return;
  }
  try {
    const publicKey = requestOptions(button.dataset.options);
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
