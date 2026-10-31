//! History: every Expense and Payment a page at a time, filtered by Member and month, with the
//! totals of each month.

use maud::{Markup, html};

use crate::domain::finance::history::{History, HistoryFilter, Month};
use crate::domain::members::{Member, MemberName};
use crate::views::finance::{Paths, crowns, entry_row, problem};
use crate::views::layout::shell;
use crate::views::nav::{Place, Tab};

/// What the filter form shows: the filter the list was made with, and the month as typed with
/// what was wrong with it.
#[derive(Debug, Clone)]
pub struct Filters<'a> {
    pub applied: HistoryFilter,
    pub month: &'a str,
    pub month_problem: Option<&'a str>,
}

impl Filters<'_> {
    /// The form for `applied`, with nothing wrong.
    #[must_use]
    pub const fn of(applied: HistoryFilter, month: &str) -> Filters<'_> {
        Filters {
            applied,
            month,
            month_problem: None,
        }
    }
}

/// History at `path` with `filter` applied, on page `page`.
fn href(path: &str, filter: HistoryFilter, page: usize) -> String {
    let mut query = Vec::new();
    if let Some(member) = filter.member {
        query.push(format!("member={}", member.get()));
    }
    if let Some(month) = filter.month {
        query.push(format!("month={month}"));
    }
    if page > 1 {
        query.push(format!("page={page}"));
    }
    if query.is_empty() {
        path.to_owned()
    } else {
        format!("{path}?{}", query.join("&"))
    }
}

/// A month the way the totals name it.
fn month_name(month: Month) -> String {
    month.first_day().strftime("%B %Y").to_string()
}

/// The History page for `me`.
///
/// It shows the filter form with every one of `members` to choose from, a page of entries with
/// the controls for the ones `me` may change, the way to the other pages, and the monthly totals
/// of every entry the filter lets through.
#[must_use]
pub fn page(
    me: &Member,
    members: &[MemberName],
    filters: &Filters,
    history: &History,
    paths: Paths,
) -> Markup {
    let applied = filters.applied;
    let filtered = !applied.is_empty() || filters.month_problem.is_some();
    shell(
        Place::Tab(Tab::Finance),
        &html! {
            h1 { "History" }

            form.history-filters method="get" action=(paths.history) {
                div.field {
                    label for="history-member" { "Member" }
                    select #history-member name="member" {
                        option value="" selected[applied.member.is_none()] { "Everyone" }
                        @for member in members {
                            option value=(member.id.get()) selected[applied.member == Some(member.id)] {
                                (member.display_name)
                            }
                        }
                    }
                }
                div.field {
                    label for="history-month" { "Month" }
                    input #history-month type="month" name="month" value=(filters.month)
                        placeholder="YYYY-MM"
                        aria-invalid=[filters.month_problem.is_some().then_some("true")]
                        aria-describedby=[filters.month_problem.is_some().then_some("history-month-problem")];
                    (problem("history-month-problem", filters.month_problem))
                }
                div.history-filter-actions {
                    button.button type="submit" { "Show" }
                    @if filtered {
                        a.button.button-secondary href=(paths.history) { "Clear" }
                    }
                }
            }

            section.finance-section aria-labelledby="history-entries" {
                h2 #history-entries { "Entries" }
                @if history.entries.is_empty() {
                    p.lead-in {
                        @if applied.is_empty() {
                            "Nothing logged yet."
                        } @else {
                            "No entries match. Try another Member or month."
                        }
                    }
                } @else {
                    ul.entry-list {
                        @for entry in &history.entries {
                            (entry_row(me, entry, paths))
                        }
                    }
                }
                @if history.pages > 1 {
                    nav.pages aria-label="Pages" {
                        @if history.page > 1 {
                            a.button.button-small rel="prev"
                                href=(href(paths.history, applied, history.page - 1)) { "Newer" }
                        }
                        span.pages-at { "Page " (history.page) " of " (history.pages) }
                        @if history.page < history.pages {
                            a.button.button-small rel="next"
                                href=(href(paths.history, applied, history.page + 1)) { "Older" }
                        }
                    }
                }
            }

            @if !history.totals.is_empty() {
                section.finance-section aria-labelledby="monthly-totals" {
                    h2 #monthly-totals { "Monthly totals" }
                    table.monthly-totals {
                        thead {
                            tr {
                                th scope="col" { "Month" }
                                th scope="col" { "Expenses" }
                                th scope="col" { "Payments" }
                            }
                        }
                        tbody {
                            @for total in &history.totals {
                                @let month = HistoryFilter { month: Some(total.month), ..applied };
                                tr {
                                    th scope="row" {
                                        a href=(href(paths.history, month, 1)) { (month_name(total.month)) }
                                    }
                                    td { (crowns(total.expenses)) }
                                    td { (crowns(total.payments)) }
                                }
                            }
                        }
                    }
                }
            }
        },
    )
}

/// History pages for the gallery and the snapshots.
#[cfg(any(test, debug_assertions))]
pub mod fixtures {
    use crate::domain::finance::history::{History, HistoryFilter, Month, MonthlyTotal};
    use crate::domain::finance::money::Crowns;
    use crate::views::finance::fixtures::{ONDRA, overview};

    /// October 2026, the month the fixture entries were logged in.
    ///
    /// # Panics
    ///
    /// Never: the fixture month is valid.
    #[must_use]
    pub fn october() -> Month {
        Month::parse("2026-10").expect("the fixture month is valid")
    }

    /// The first of three pages, with the latest fixture entries and three months' totals.
    ///
    /// # Panics
    ///
    /// If a fixture value is invalid, which is a bug in the fixtures.
    #[must_use]
    pub fn unfiltered() -> History {
        let month = |text| Month::parse(text).expect("fixture months are valid");
        History {
            entries: overview().recent,
            page: 1,
            pages: 3,
            totals: vec![
                MonthlyTotal {
                    month: october(),
                    expenses: Crowns::new(14_100),
                    payments: Crowns::new(4_160),
                },
                MonthlyTotal {
                    month: month("2026-09"),
                    expenses: Crowns::new(2_315),
                    payments: Crowns::ZERO,
                },
                MonthlyTotal {
                    month: month("2026-08"),
                    expenses: Crowns::new(38_900),
                    payments: Crowns::new(12_000),
                },
            ],
        }
    }

    /// Ondra's entries in October.
    #[must_use]
    pub fn ondra_in_october() -> HistoryFilter {
        HistoryFilter {
            member: Some(ONDRA),
            month: Some(october()),
        }
    }

    /// [`ondra_in_october`]'s one page: the Payment he received and the Expense he paid.
    ///
    /// # Panics
    ///
    /// If a fixture value is invalid, which is a bug in the fixtures.
    #[must_use]
    pub fn filtered() -> History {
        History {
            entries: overview().recent.into_iter().take(2).collect(),
            page: 1,
            pages: 1,
            totals: vec![MonthlyTotal {
                month: october(),
                expenses: Crowns::new(12_480),
                payments: Crowns::new(4_160),
            }],
        }
    }

    /// Nothing the filter lets through.
    #[must_use]
    pub const fn empty() -> History {
        History {
            entries: Vec::new(),
            page: 1,
            pages: 1,
            totals: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{empty, filtered, october, ondra_in_october, unfiltered};
    use super::*;
    use crate::views::assert_html_snapshot;
    use crate::views::finance::fixtures::{JOSEF, ONDRA, PATHS, SIMON, crew, member};

    #[test]
    fn history_unfiltered() {
        assert_html_snapshot!(page(
            &member(ONDRA),
            &crew(),
            &Filters::of(HistoryFilter::default(), ""),
            &unfiltered(),
            PATHS
        ));
    }

    #[test]
    fn history_filtered() {
        assert_html_snapshot!(page(
            &member(ONDRA),
            &crew(),
            &Filters::of(ondra_in_october(), "2026-10"),
            &filtered(),
            PATHS
        ));
    }

    #[test]
    fn history_empty() {
        let filter = HistoryFilter {
            member: Some(SIMON),
            month: Some(october()),
        };
        assert_html_snapshot!(page(
            &member(SIMON),
            &crew(),
            &Filters::of(filter, "2026-10"),
            &empty(),
            PATHS
        ));
    }

    #[test]
    fn history_with_a_mistyped_month() {
        let filters = Filters {
            applied: HistoryFilter::default(),
            month: "Oct",
            month_problem: Some("Enter a month as YYYY-MM, like 2026-10."),
        };
        assert_html_snapshot!(page(
            &member(JOSEF),
            &crew(),
            &filters,
            &unfiltered(),
            PATHS
        ));
    }

    #[test]
    fn nothing_logged_says_so_without_a_way_to_clear() {
        let html = page(
            &member(JOSEF),
            &crew(),
            &Filters::of(HistoryFilter::default(), ""),
            &empty(),
            PATHS,
        )
        .into_string();

        assert!(html.contains("Nothing logged yet."));
        assert!(!html.contains(">Clear</a>"));
        assert!(!html.contains("Monthly totals"));
        assert!(!html.contains("aria-label=\"Pages\""));
    }

    #[test]
    fn the_page_links_keep_the_filter() {
        let history = History {
            page: 2,
            ..unfiltered()
        };
        let html = page(
            &member(JOSEF),
            &crew(),
            &Filters::of(ondra_in_october(), "2026-10"),
            &history,
            PATHS,
        )
        .into_string();

        assert!(html.contains("rel=\"prev\" href=\"/history?member=2&amp;month=2026-10\""));
        assert!(
            html.contains("rel=\"next\" href=\"/history?member=2&amp;month=2026-10&amp;page=3\"")
        );
        assert!(html.contains("Page 2 of 3"));
    }

    #[test]
    fn each_month_leads_to_its_entries_for_the_same_member() {
        let filter = HistoryFilter {
            member: Some(ONDRA),
            month: None,
        };
        let html = page(
            &member(JOSEF),
            &crew(),
            &Filters::of(filter, ""),
            &unfiltered(),
            PATHS,
        )
        .into_string();

        assert!(html.contains(
            "<a href=\"/history?member=2&amp;month=2026-09\">September 2026</a></th><td>2\u{a0}315\u{a0}Kč</td><td>0\u{a0}Kč</td>"
        ));
        assert!(html.contains("<option value=\"2\" selected>Ondra</option>"));
    }

    #[test]
    fn the_rows_carry_the_controls_of_the_entries_i_may_change() {
        let html = page(
            &member(ONDRA),
            &crew(),
            &Filters::of(HistoryFilter::default(), ""),
            &unfiltered(),
            PATHS,
        )
        .into_string();

        // Ondra paid the Malt, but Josef sent the Payment.
        assert!(html.contains("href=\"/expenses/7/edit\""));
        assert!(!html.contains("href=\"/payments/4/edit\""));
    }
}
