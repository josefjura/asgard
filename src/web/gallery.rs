use maud::Markup;

use crate::views::gallery;

/// The component gallery, compiled into debug builds only.
pub async fn show() -> Markup {
    gallery::page(&gallery::fixtures())
}
