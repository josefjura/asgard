// Runs the WebAuthn ceremony behind a passkey button. The server sends the options and checks
// the result; this module only moves bytes between it and the browser's authenticator.
//
// A button with `data-passkey` set to `register` or `sign-in` runs that ceremony through its
// `data-passkey-start` and `data-passkey-finish` URLs. If a link stops working meanwhile, the
// page reloads to show why.

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
  if (response.status === 410) {
    throw new LinkGone();
  }
  if (!response.ok) {
    // Our error responses are a sentence meant for the person.
    throw new ServerError(await response.text());
  }
  return response.json();
}

class ServerError extends Error {}
class LinkGone extends Error {}

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

// The options also ask for conditional mediation (autofill), which a button doesn't use, so
// only `publicKey` is passed on and the browser shows its usual passkey prompt.
async function getCredential({ publicKey }) {
  publicKey.challenge = fromBase64Url(publicKey.challenge);
  for (const credential of publicKey.allowCredentials ?? []) {
    credential.id = fromBase64Url(credential.id);
  }
  const credential = await navigator.credentials.get({ publicKey });
  const { authenticatorData, clientDataJSON, signature, userHandle } = credential.response;
  return {
    id: credential.id,
    rawId: toBase64Url(credential.rawId),
    type: credential.type,
    response: {
      authenticatorData: toBase64Url(authenticatorData),
      clientDataJSON: toBase64Url(clientDataJSON),
      signature: toBase64Url(signature),
      userHandle: userHandle ? toBase64Url(userHandle) : null,
    },
    extensions: credential.getClientExtensionResults(),
  };
}

const ceremonies = {
  register: {
    credential: createCredential,
    cancelled: "The passkey wasn't created. Press the button to try again.",
  },
  "sign-in": {
    credential: getCredential,
    cancelled: "You weren't signed in. Press the button to try again.",
  },
};

// Browser errors carry developer text, so they become sentences a person can act on.
function message(error, ceremony) {
  if (error instanceof ServerError) return error.message;
  console.error(error);
  switch (error.name) {
    case "NotAllowedError":
    case "AbortError":
      return ceremony.cancelled;
    case "InvalidStateError":
      return "This device already has a passkey for Asgard.";
    case "NotSupportedError":
      return "This browser can't use passkeys. Try another browser or device.";
    default:
      return "Something went wrong with the passkey. Please try again.";
  }
}

for (const button of document.querySelectorAll("[data-passkey]")) {
  const { passkey, passkeyStart, passkeyFinish } = button.dataset;
  const ceremony = ceremonies[passkey];
  const status = document.getElementById(button.getAttribute("aria-describedby"));
  button.addEventListener("click", async () => {
    button.disabled = true;
    status.textContent = "";
    try {
      const { ceremony: id, options } = await post(passkeyStart, {});
      const credential = await ceremony.credential(options);
      const { redirect } = await post(passkeyFinish, { ceremony: id, credential });
      window.location.assign(redirect);
    } catch (error) {
      if (error instanceof LinkGone) {
        window.location.reload();
        return;
      }
      status.textContent = message(error, ceremony);
      button.disabled = false;
    }
  });
}
