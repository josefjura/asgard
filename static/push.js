// Turns notifications on and off for this device. The server renders the control with every
// state in it and shows the one it knows: whether this session is subscribed. This module then
// shows the one that's true in this browser, and runs the buttons.
//
// The control carries the VAPID public key in `data-push-key` and the URLs it posts to in
// `data-push-subscribe` and `data-push-unsubscribe`.

const control = document.getElementById("push-control");
const { pushKey, pushSubscribe, pushUnsubscribe } = control.dataset;
const status = control.querySelector("[role=status]");

// iPadOS asks for desktop sites, so an iPad says it is a Mac with a touch screen.
const ios =
  /iPhone|iPad|iPod/.test(navigator.userAgent) ||
  (/Macintosh/.test(navigator.userAgent) && navigator.maxTouchPoints > 1);
const installed =
  window.matchMedia("(display-mode: standalone)").matches || navigator.standalone === true;
const supported = "serviceWorker" in navigator && "PushManager" in window && "Notification" in window;

const fromBase64Url = (text) =>
  Uint8Array.from(atob(text.replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0));

const sameBytes = (buffer, bytes) => {
  const other = new Uint8Array(buffer);
  return other.length === bytes.length && other.every((byte, i) => byte === bytes[i]);
};

class ServerError extends Error {}
class Unsupported extends Error {}

function shown() {
  return control.querySelector("[data-push-state]:not([hidden])")?.dataset.pushState;
}

function show(state) {
  for (const part of control.querySelectorAll("[data-push-state]")) {
    part.hidden = part.dataset.pushState !== state;
  }
}

async function post(url, body) {
  const response = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  // A signed-out request is sent to sign in, and fetch follows it.
  if (response.redirected) {
    window.location.assign(response.url);
    throw new ServerError("");
  }
  if (!response.ok) {
    // Our error responses are a sentence meant for the person.
    throw new ServerError(await response.text());
  }
}

async function pushManager() {
  return (await navigator.serviceWorker.ready).pushManager;
}

// The browser's subscription, unless it was made with another key: one made with an old key
// can't receive our pushes, so it is dropped.
async function browserSubscription(manager, key) {
  const subscription = await manager.getSubscription();
  const used = subscription?.options?.applicationServerKey;
  if (used && !sameBytes(used, key)) {
    await subscription.unsubscribe();
    return null;
  }
  return subscription;
}

async function turnOn() {
  if (!supported) throw new Unsupported();
  const permission = await Notification.requestPermission();
  if (permission === "denied") {
    show("blocked");
    return;
  }
  if (permission !== "granted") {
    status.textContent = "Notifications weren't turned on. Press the button to try again.";
    return;
  }
  const key = fromBase64Url(pushKey);
  const manager = await pushManager();
  const subscription =
    (await browserSubscription(manager, key)) ??
    (await manager.subscribe({ userVisibleOnly: true, applicationServerKey: key }));
  await post(pushSubscribe, subscription.toJSON());
  show("on");
}

// The server forgets the device first, so the Crew's pushes stop even if the browser fails;
// a browser that fails then only keeps a subscription nobody sends to.
async function turnOff() {
  await post(pushUnsubscribe, {});
  show("off");
  if (supported) {
    try {
      await (await (await pushManager()).getSubscription())?.unsubscribe();
    } catch (error) {
      console.warn("The browser kept its push subscription", error);
    }
  }
}

// Browser errors carry developer text, so they become sentences a person can act on.
function message(error) {
  if (error instanceof ServerError) return error.message;
  if (error instanceof Unsupported) {
    return "This browser can't show notifications. Try another browser, or install Asgard.";
  }
  console.error(error);
  return "Something went wrong with notifications. Please try again.";
}

// What's true in this browser, starting from what the server knows. When the server has this
// device, it is brought up to date: forgotten if the browser can no longer show its pushes,
// sent the browser's subscription again otherwise, in case the push service changed it.
async function settle() {
  const serverHasIt = shown() === "on";
  if (ios && !installed) return show("install");
  if (!supported) return show("off");
  if (Notification.permission === "denied") {
    show("blocked");
    if (serverHasIt) await post(pushUnsubscribe, {});
    return;
  }
  if (!serverHasIt) return show("off");
  const subscription = await browserSubscription(await pushManager(), fromBase64Url(pushKey));
  if (subscription) {
    await post(pushSubscribe, subscription.toJSON());
  } else {
    show("off");
    await post(pushUnsubscribe, {});
  }
}

for (const button of control.querySelectorAll("[data-push-action]")) {
  const action = button.dataset.pushAction === "on" ? turnOn : turnOff;
  button.addEventListener("click", async () => {
    button.disabled = true;
    status.textContent = "";
    try {
      await action();
    } catch (error) {
      status.textContent = message(error);
    } finally {
      button.disabled = false;
    }
  });
}

settle().catch((error) => console.error(error));
