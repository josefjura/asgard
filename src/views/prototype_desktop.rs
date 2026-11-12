//! PROTOTYPE, throw away (#146): three desktop shells, with Finance and Shopping in them.
//!
//! Every variant is CSS alone over today's markup, under `html[data-proto=…]` and the 48rem
//! breakpoint, so the phone layout never changes. On a real page the inline script sets the
//! attribute from `?variant=a|b|c` and draws the switcher; the gallery's iframes run no scripts,
//! so the gallery stamps the attribute into each fixture instead. Debug builds only.

use maud::{Markup, PreEscaped, html};

/// The variants, in switcher order: the key `?variant=` takes, and its name.
pub const VARIANTS: [(&str, &str); 6] = [
    ("today", "Today"),
    ("a", "A: Wide band, aside on the right"),
    ("b", "B: Side rail, three columns"),
    ("b2", "B2: Side rail, content centred beside it"),
    ("b3", "B3: Side rail and content centred together"),
    ("c", "C: Summary panel on the left"),
];

const CSS: &str = r#"
@media (min-width: 48rem) {
  /* ---------- A: today's band, a wider column, main with an aside on the right ---------- */
  html[data-proto="a"] { --column: 72rem; }

  html[data-proto="a"] .shell-main:has(.my-balance) {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 22rem;
    grid-template-rows: auto auto auto 1fr;
    grid-template-areas: "title crew" "balance crew" "actions crew" "latest crew";
    gap: 0 var(--space-5);
    align-items: start;
  }
  html[data-proto="a"] .shell-main:has(.my-balance) > h1 { grid-area: title; }
  html[data-proto="a"] .my-balance { grid-area: balance; }
  html[data-proto="a"] .finance-actions { grid-area: actions; max-width: 28rem; }
  html[data-proto="a"] .shell-main .columns { display: contents; }
  html[data-proto="a"] [aria-labelledby="latest-entries"] { grid-area: latest; }
  html[data-proto="a"] [aria-labelledby="crew-balances"] {
    grid-area: crew;
    position: sticky;
    top: var(--space-4);
    margin-top: var(--space-2);
    padding: var(--space-3);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--surface);
  }

  html[data-proto="a"] .shell-main:has(#open-items) {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 22rem;
    grid-template-rows: auto auto 1fr;
    grid-template-areas: "title title" "list form" "list bought";
    gap: 0 var(--space-5);
    align-items: start;
  }
  html[data-proto="a"] .shell-main:has(#open-items) > h1 { grid-area: title; }
  html[data-proto="a"] [aria-labelledby="new-item"] {
    grid-area: form;
    padding: var(--space-3);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--surface);
  }
  html[data-proto="a"] #open-items { grid-area: list; }
  html[data-proto="a"] #recently-bought { grid-area: bought; }

  /* ---------- B: a side rail for the navigation, three columns of content ---------- */
  html[data-proto^="b"] .shell {
    grid-template-columns: 14rem minmax(0, 1fr);
    grid-template-rows: 1fr;
  }
  html[data-proto^="b"] .shell-header {
    grid-area: 1 / 1 / 2 / 2;
    position: sticky;
    top: 0;
    height: 100dvh;
    flex-direction: column;
    align-items: flex-start;
    padding: var(--space-4) var(--space-3);
  }
  html[data-proto^="b"] .settings-link { margin: auto 0 0; }
  html[data-proto^="b"] .settings-link::before {
    inset: var(--space-2) auto var(--space-2) calc(-1 * var(--space-3));
    width: 3px;
    height: auto;
    border-radius: 0 3px 3px 0;
  }
  html[data-proto^="b"] .tabs {
    grid-area: 1 / 1 / 2 / 2;
    align-self: start;
    justify-self: stretch;
    position: sticky;
    top: 5rem;
    margin-top: 5rem;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: 0 var(--space-2);
    background: transparent;
    z-index: 1;
  }
  html[data-proto^="b"] .tab {
    justify-content: flex-start;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    font-size: 1rem;
    border-radius: var(--radius);
  }
  html[data-proto^="b"] .tab[aria-current="page"] { background: rgb(255 255 255 / 0.1); }
  html[data-proto^="b"] .tab::before {
    inset: var(--space-2) auto var(--space-2) calc(-1 * var(--space-2));
    width: 3px;
    height: auto;
    border-radius: 0 3px 3px 0;
  }
  html[data-proto^="b"] .shell-main {
    grid-area: 1 / 2 / 2 / 3;
    max-width: 80rem;
    padding: var(--space-5);
  }

  /* B2: the rail at the screen's edge, the content centred in what's left. */
  html[data-proto="b2"] .shell-main { justify-self: center; }

  /* B3: rail and content centred as one block; the band's green runs out to the left edge. */
  html[data-proto="b3"] .shell { max-width: calc(14rem + 80rem); margin-inline: auto; }
  html[data-proto="b3"] body {
    background: linear-gradient(to right, var(--band) 50%, var(--bg) 50%);
  }
  html[data-proto="b3"] .shell-main { background: var(--bg); min-height: 100dvh; }

  html[data-proto^="b"] .shell-main:has(.my-balance) {
    display: grid;
    grid-template-columns: 20rem minmax(0, 1fr) 20rem;
    grid-template-rows: auto auto 1fr;
    grid-template-areas: "title title title" "balance latest crew" "actions latest crew";
    gap: 0 var(--space-5);
    align-items: start;
  }
  html[data-proto^="b"] .shell-main:has(.my-balance) > h1 { grid-area: title; }
  html[data-proto^="b"] .my-balance { grid-area: balance; }
  html[data-proto^="b"] .finance-actions { grid-area: actions; flex-direction: column; }
  html[data-proto^="b"] .shell-main .columns { display: contents; }
  html[data-proto^="b"] [aria-labelledby="latest-entries"] { grid-area: latest; }
  html[data-proto^="b"] [aria-labelledby="crew-balances"] { grid-area: crew; }

  /* Shopping as a board: the form in one line, a column per Urgency. */
  html[data-proto^="b"] [aria-labelledby="new-item"] .entry-form {
    grid-template-columns: minmax(0, 2fr) auto minmax(0, 1fr) auto;
    align-items: end;
    max-width: none;
  }
  html[data-proto^="b"] [aria-labelledby="new-item"] .entry-form > * { margin: 0; }
  html[data-proto^="b"] #open-items {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: var(--space-4);
    align-items: start;
  }
  html[data-proto^="b"] #open-items > .section {
    margin: 0 0 var(--space-5);
    padding: var(--space-3);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--surface);
  }
  html[data-proto^="b"] #open-items > .empty { grid-column: 1 / -1; }

  /* ---------- C: the band across the screen, a summary panel on the left ---------- */
  html[data-proto="c"] { --column: 80rem; }
  html[data-proto="c"] .tabs { justify-self: start; margin-left: 10rem; }

  html[data-proto="c"] .shell-main:has(.my-balance) {
    display: grid;
    grid-template-columns: 24rem minmax(0, 1fr);
    grid-template-rows: auto auto auto 1fr;
    grid-template-areas: "title latest" "balance latest" "actions latest" "crew latest";
    gap: 0 var(--space-5);
    align-items: start;
  }
  html[data-proto="c"] .shell-main:has(.my-balance) > h1 { grid-area: title; }
  html[data-proto="c"] .my-balance { grid-area: balance; }
  html[data-proto="c"] .finance-actions { grid-area: actions; }
  html[data-proto="c"] .shell-main .columns { display: contents; }
  html[data-proto="c"] [aria-labelledby="latest-entries"] {
    grid-area: latest;
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--surface);
  }
  html[data-proto="c"] [aria-labelledby="crew-balances"] { grid-area: crew; }

  html[data-proto="c"] .shell-main:has(#open-items) {
    display: grid;
    grid-template-columns: 24rem minmax(0, 1fr);
    grid-template-rows: auto auto 1fr;
    grid-template-areas: "title list" "form list" "form bought";
    gap: 0 var(--space-5);
    align-items: start;
  }
  html[data-proto="c"] .shell-main:has(#open-items) > h1 { grid-area: title; }
  html[data-proto="c"] [aria-labelledby="new-item"] {
    grid-area: form;
    position: sticky;
    top: var(--space-4);
  }
  html[data-proto="c"] #open-items {
    grid-area: list;
    padding: var(--space-3) var(--space-4) 0;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--surface);
  }
  html[data-proto="c"] #recently-bought { grid-area: bought; margin-top: var(--space-4); }
}

/* The switcher: not part of any design. */
.proto-bar {
  position: fixed;
  bottom: 1rem;
  left: 50%;
  transform: translateX(-50%);
  z-index: 99;
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.375rem 0.5rem;
  border-radius: 999px;
  background: #e0218a;
  color: #fff;
  font: 600 0.875rem system-ui, sans-serif;
  box-shadow: 0 4px 16px rgb(0 0 0 / 0.35);
}
.proto-bar button {
  min-height: 0;
  width: 2rem;
  height: 2rem;
  border: 0;
  border-radius: 999px;
  background: rgb(255 255 255 / 0.2);
  color: inherit;
  font: inherit;
  cursor: pointer;
}
.proto-bar span { padding: 0 0.5rem; white-space: nowrap; }
"#;

/// Sets `data-proto` from `?variant=` before the body renders, then draws the switcher. The
/// arrows and the arrow keys reload with the next variant, so the gallery can re-stamp.
const SCRIPT: &str = r#"
(() => {
  const variants = __VARIANTS__;
  const params = new URLSearchParams(location.search);
  const current = variants.findIndex(([key]) => key === (params.get("variant") || "today"));
  const at = current < 0 ? 0 : current;
  if (at > 0) document.documentElement.dataset.proto = variants[at][0];
  const go = (step) => {
    const next = variants[(at + step + variants.length) % variants.length][0];
    if (next === "today") params.delete("variant"); else params.set("variant", next);
    const query = params.toString();
    location.search = query ? "?" + query : "";
  };
  addEventListener("keydown", (event) => {
    if (event.target.closest("input, textarea, select, [contenteditable]")) return;
    if (event.key === "ArrowLeft") go(-1);
    if (event.key === "ArrowRight") go(1);
  });
  addEventListener("DOMContentLoaded", () => {
    const bar = document.createElement("div");
    bar.className = "proto-bar";
    bar.innerHTML = '<button type="button" aria-label="Previous variant">&larr;</button>'
      + "<span></span>"
      + '<button type="button" aria-label="Next variant">&rarr;</button>';
    bar.querySelector("span").textContent = "Prototype · " + variants[at][1];
    const [previous, next] = bar.querySelectorAll("button");
    previous.onclick = () => go(-1);
    next.onclick = () => go(1);
    document.body.append(bar);
  });
})();
"#;

/// The variant CSS and the switcher script, for the head of every document.
#[must_use]
pub fn head() -> Markup {
    let variants = VARIANTS
        .iter()
        .map(|(key, name)| format!("[\"{key}\",\"{name}\"]"))
        .collect::<Vec<_>>()
        .join(",");
    html! {
        style { (PreEscaped(CSS)) }
        script { (PreEscaped(SCRIPT.replace("__VARIANTS__", &format!("[{variants}]")))) }
    }
}

/// `document` with `variant` stamped on its root, for the gallery's script-less frames.
#[must_use]
pub fn stamp(document: &Markup, variant: &str) -> Markup {
    if !VARIANTS.iter().skip(1).any(|(key, _)| *key == variant) {
        return document.clone();
    }
    PreEscaped(document.0.replacen(
        "<html lang=\"en\">",
        &format!("<html lang=\"en\" data-proto=\"{variant}\">"),
        1,
    ))
}
