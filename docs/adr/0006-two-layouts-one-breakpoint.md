# Two designed layouts, one breakpoint, same HTML

Asgard has a phone layout and a desktop layout, each designed on its own terms, and serves the same HTML to both: CSS switches between them at a single app-wide breakpoint. We considered splitting by device (the server sniffing the User-Agent or Client Hints and rendering separate templates) and rejected it: every page and test would exist twice and drift apart, the server only guesses at "mobile" and never knows the window's width, so a resized desktop window or a tablet gets the wrong version, and the service worker's cache would have to vary by device. Fluid layouts that morph across many widths were rejected too; they are what made responsive sites hard to support.

## Consequences

- Pages are composed from a small set of layout primitives in `views/` (single column, main with an aside, list with detail, card grid). A page picks one and adds no media queries of its own.
- Every region a layout needs is always in the markup; on a phone CSS stacks or hides what the desktop shows side by side. A list-and-detail URL renders both, so one URL works at both widths and insta snapshots stay single.
- Tablets get whichever layout their width falls into; there is no third design.
- UI changes are reviewed as gallery screenshots at phone and desktop width.
- Work that only makes sense at a desk becomes its own page at its own URL, not a desktop variant of an existing one.
