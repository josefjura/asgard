// Runs the WebAuthn ceremony behind a passkey button. The server sends the options and checks
// the result; this module only moves bytes between it and the browser's authenticator.
//
// A button with `data-passkey-join="<invite path>"` registers a passkey through that Invite link.

const fromBase64Url = (text) =>
  Uint8Array.from(atob(text.replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0)).buffer;

const toBase64Url = (buffer) =>
  btoa(String.fromCharCode(...new Uint8Array(buffer)))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");

async function post(url, body) {
  const response = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!response.ok) {
    // Our error responses are a sentence meant for the person.
    throw new ServerError(await response.text());
  }
  return response.json();
}

class ServerError extends Error {}

async function createCredential({ publicKey }) {
  publicKey.challenge = fromBase64Url(publicKey.challenge);
  publicKey.user.id = fromBase64Url(publicKey.user.id);
  for (const credential of publicKey.excludeCredentials ?? []) {
    credential.id = fromBase64Url(credential.id);
  }
  const credential = await navigator.credentials.create({ publicKey });
  return {
    id: credential.id,
    rawId: toBase64Url(credential.rawId),
    type: credential.type,
    response: {
      attestationObject: toBase64Url(credential.response.attestationObject),
      clientDataJSON: toBase64Url(credential.response.clientDataJSON),
    },
    extensions: credential.getClientExtensionResults(),
  };
}

// Browser errors carry developer text, so they become sentences a person can act on.
function message(error) {
  if (error instanceof ServerError) return error.message;
  console.error(error);
  switch (error.name) {
    case "NotAllowedError":
    case "AbortError":
      return "The passkey wasn't created. Press the button to try again.";
    case "InvalidStateError":
      return "This device already has a passkey for Asgard.";
    case "NotSupportedError":
      return "This browser can't create passkeys. Try another browser or device.";
    default:
      return "Something went wrong with the passkey. Please try again.";
  }
}

for (const button of document.querySelectorAll("[data-passkey-join]")) {
  const path = button.dataset.passkeyJoin;
  const status = document.getElementById(button.getAttribute("aria-describedby"));
  button.addEventListener("click", async () => {
    button.disabled = true;
    status.textContent = "";
    try {
      const { ceremony, options } = await post(`${path}/start`, {});
      const credential = await createCredential(options);
      const { redirect } = await post(`${path}/finish`, { ceremony, credential });
      window.location.assign(redirect);
    } catch (error) {
      status.textContent = message(error);
      button.disabled = false;
    }
  });
}
