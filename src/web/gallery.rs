use std::collections::HashMap;

use axum::extract::Query;
use maud::Markup;

use crate::views::{gallery, prototype_desktop};

/// The component gallery, compiled into debug builds only. PROTOTYPE (#146): `?variant=` stamps
/// a desktop variant into every fixture.
pub async fn show(Query(query): Query<HashMap<String, String>>) -> Markup {
    let variant = query.get("variant").map_or("today", String::as_str);
    let fixtures: Vec<gallery::Fixture> = gallery::fixtures()
        .into_iter()
        .map(|fixture| gallery::Fixture {
            document: prototype_desktop::stamp(&fixture.document, variant),
            ..fixture
        })
        .collect();
    gallery::page(&fixtures)
}
