# Server-rendered Maud + htmx, no JavaScript toolchain

Asgard is an app of forms and lists for three people, mostly written by Claude and reviewed by a human, so compile-time checking matters more than client richness. Every page is rendered on the server with Maud (a broken template fails `cargo build`) and htmx swaps fragments. JavaScript is limited to small hand-written modules for browser APIs, with no npm or bundler. A `/dev/gallery` route plus `insta` snapshots replaces Storybook.

## Considered Options

- **React / Next.js:** heavy, and server components add a JS layer we don't need.
- **SvelteKit, Leptos:** client state machinery a forms-and-lists app doesn't need.
- **Lit + Storybook as the component model:** splits the UI into Rust and JS halves, and Storybook sees only the JS half.
- **Askama:** separate HTML files help only if non-Rust people edit the markup.
- **Datastar:** promising, but htmx is mature and already familiar.
- **Go + templ:** faster builds but less type safety; no reason to leave Rust.
