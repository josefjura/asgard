//! `import-heimdall` end to end: a Heimdall database built from Heimdall's real schema, imported
//! into a fresh Asgard, and checked through the report and what Asgard's services and pages show.

use axum::http::StatusCode;
use sqlx::SqlitePool;
use url::Url;

use super::{ImportFailure, ImportRun, import_heimdall};
use crate::domain::finance::history::HistoryFilter;
use crate::domain::heimdall::fixtures::{HeimdallBuilder, HeimdallFile};
use crate::domain::heimdall::{EntryKind, ImportError, ImportOptions, Rename, Rounding};
use crate::domain::members::MemberName;
use crate::domain::quotes::Search;
use crate::web::test_support::{PUBLIC_ORIGIN, TestApp, authenticator, body_text, cookie_pair};

fn options(admin: &str) -> ImportOptions {
    ImportOptions {
        admin: admin.to_owned(),
        renames: Vec::new(),
        dry_run: false,
    }
}

fn dry_run(admin: &str) -> ImportOptions {
    ImportOptions {
        dry_run: true,
        ..options(admin)
    }
}

async fn run(
    app: &TestApp,
    heimdall: &HeimdallFile,
    options: &ImportOptions,
) -> Result<ImportRun, ImportFailure> {
    import_heimdall(
        &app.state.db,
        &app.state.clock,
        &app.state.auth,
        &Url::parse(PUBLIC_ORIGIN).unwrap(),
        heimdall.path(),
        options,
    )
    .await
}

async fn import_error(
    app: &TestApp,
    heimdall: &HeimdallFile,
    options: &ImportOptions,
) -> ImportError {
    match run(app, heimdall, options).await {
        Err(ImportFailure::Import(err)) => err,
        Err(err) => panic!("expected the import to stop, got {err}"),
        Ok(run) => panic!("expected the import to stop, got\n{}", run.imported.report),
    }
}

async fn names(app: &TestApp) -> Vec<String> {
    app.state
        .members
        .all()
        .await
        .unwrap()
        .into_iter()
        .map(|member: MemberName| member.display_name.to_string())
        .collect()
}

/// Each Member's Balance in crowns, in the order they joined.
async fn balances(app: &TestApp) -> Vec<(String, i64)> {
    app.state
        .finance
        .overview()
        .await
        .unwrap()
        .balances
        .into_iter()
        .map(|row| (row.member.display_name.to_string(), row.balance.get()))
        .collect()
}

/// Nothing anyone could see in Asgard: no Members, entries, Reminders, Quotes or links.
async fn assert_asgard_is_empty(app: &TestApp) {
    assert!(names(app).await.is_empty(), "Members were left behind");
    let history = app
        .state
        .finance
        .history(&HistoryFilter::default(), 1)
        .await
        .unwrap();
    assert!(history.entries.is_empty(), "entries were left behind");
    assert_eq!(app.state.reminders.pending().await.unwrap().len(), 0);
    assert_eq!(app.state.reminders.sent().await.unwrap().len(), 0);
    let quotes = app
        .state
        .quotes
        .page(&Search::everything(), 1)
        .await
        .unwrap();
    assert!(quotes.quotes.is_empty(), "Quotes were left behind");
    assert_eq!(app.state.auth.open_links().await.unwrap().len(), 0);
}

/// Josef, Ondra and Pepa with a few months of brewing: Josef paid 300 and Pepa 90, and Ondra sent
/// Josef 100.
async fn brewing() -> HeimdallBuilder {
    let heimdall = HeimdallBuilder::crew().await;
    heimdall
        .expense(1, 300.0, "Slad a chmel", "2025-09-14 10:00:00")
        .await;
    heimdall
        .expense(3, 90.0, "Kvasnice", "2025-10-02 17:30:00")
        .await;
    heimdall
        .payment(2, 1, 100.0, "Vyrovnání", "2025-10-20 08:00:00")
        .await;
    heimdall
}

#[sqlx::test]
async fn imports_members_expenses_and_payments_and_the_ledger_agrees(db: SqlitePool) {
    let app = TestApp::new(db);
    let heimdall = brewing().await.finish().await;

    let run = run(&app, &heimdall, &options("Josef")).await.unwrap();

    assert!(run.imported.committed);
    assert!(run.imported.report.passed(), "{}", run.imported.report);
    assert_eq!(names(&app).await, ["Josef", "Ondra", "Pepa"]);
    // Josef: +200 for his Expense, -30 for Pepa's, -100 received. Ondra: -100 -30 +100.
    assert_eq!(
        balances(&app).await,
        [
            ("Josef".to_owned(), 70),
            ("Ondra".to_owned(), -30),
            ("Pepa".to_owned(), -40)
        ]
    );
    let overview = app.state.finance.overview().await.unwrap();
    let total_settled: i64 = overview
        .settlements
        .iter()
        .map(|settlement| settlement.amount.get())
        .sum();
    assert_eq!(total_settled, 70);
    let history = app
        .state
        .finance
        .history(&HistoryFilter::default(), 1)
        .await
        .unwrap();
    assert_eq!(history.entries.len(), 3);
}

#[sqlx::test]
async fn the_report_compares_counts_totals_and_balances(db: SqlitePool) {
    let app = TestApp::new(db);
    let heimdall = brewing().await.finish().await;

    let report = run(&app, &heimdall, &options("Josef"))
        .await
        .unwrap()
        .imported
        .report;

    let counts: Vec<_> = report
        .counts
        .iter()
        .map(|row| (row.kind, row.heimdall, row.asgard))
        .collect();
    assert_eq!(
        counts,
        [
            (EntryKind::Expense, 2, 2),
            (EntryKind::Payment, 1, 1),
            (EntryKind::Reminder, 0, 0),
            (EntryKind::Quote, 0, 0)
        ]
    );
    let totals: Vec<_> = report
        .totals
        .iter()
        .map(|row| (row.kind, row.heimdall, row.asgard))
        .collect();
    assert_eq!(
        totals,
        [
            (EntryKind::Expense, 390, 390),
            (EntryKind::Payment, 100, 100)
        ]
    );
    let balances: Vec<_> = report
        .balances
        .iter()
        .map(|row| {
            (
                row.member.to_string(),
                row.heimdall_hundredths,
                row.asgard.get(),
            )
        })
        .collect();
    assert_eq!(
        balances,
        [
            ("Josef".to_owned(), 7000, 70),
            ("Ondra".to_owned(), -3000, -30),
            ("Pepa".to_owned(), -4000, -40)
        ]
    );
    let printed = report.to_string();
    assert!(
        printed.contains("Verdict: PASS. The import is committed."),
        "{printed}"
    );
}

#[sqlx::test]
async fn renames_members_and_makes_the_named_one_admin(db: SqlitePool) {
    let app = TestApp::new(db);
    let heimdall = HeimdallBuilder::new().await;
    heimdall.assign(1, "josef_j").await;
    heimdall.assign(2, "ondrej_k").await;
    heimdall.assign(3, "Pepa").await;
    let heimdall = heimdall.finish().await;
    let options = ImportOptions {
        renames: vec![
            Rename {
                from: "josef_j".to_owned(),
                to: "Josef".to_owned(),
            },
            Rename {
                from: "ondrej_k".to_owned(),
                to: "Ondra".to_owned(),
            },
        ],
        ..options("ondra")
    };

    let run = run(&app, &heimdall, &options).await.unwrap();

    assert_eq!(names(&app).await, ["Josef", "Ondra", "Pepa"]);
    let ondra = app
        .state
        .members
        .find(run.imported.admin)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ondra.display_name.as_str(), "Ondra");
    assert!(ondra.is_admin);
    assert_eq!(
        app.state
            .members
            .admin_name()
            .await
            .unwrap()
            .unwrap()
            .as_str(),
        "Ondra"
    );
}

#[sqlx::test]
async fn a_dry_run_reports_and_commits_nothing(db: SqlitePool) {
    let app = TestApp::new(db);
    let heimdall = brewing().await;
    heimdall
        .reminder(
            1,
            "Stočit",
            "2026-02-01T09:00:00+00:00",
            "2025-12-01 10:00:00",
        )
        .await;
    heimdall
        .quote("Pivo je základ.", "Pepa", "2025-11-11 11:11:11")
        .await;
    let heimdall = heimdall.finish().await;

    let run = run(&app, &heimdall, &dry_run("Josef")).await.unwrap();

    assert!(!run.imported.committed);
    assert!(run.imported.report.passed());
    let printed = run.imported.report.to_string();
    assert!(printed.contains("Balances (Kč)"), "{printed}");
    assert!(
        printed.contains("Dry run, so nothing was committed."),
        "{printed}"
    );
    assert_eq!(run.device_links.len(), 0);
    assert_asgard_is_empty(&app).await;
}

#[sqlx::test]
async fn refuses_an_asgard_that_already_has_members(db: SqlitePool) {
    let app = TestApp::new(db);
    app.sign_in_new_admin("Josef").await;
    let heimdall = brewing().await.finish().await;

    let err = import_error(&app, &heimdall, &options("Josef")).await;

    assert!(matches!(err, ImportError::NotEmpty), "{err}");
    assert_eq!(names(&app).await, ["Josef"]);
    assert!(
        balances(&app)
            .await
            .iter()
            .all(|(_, balance)| *balance == 0)
    );
}

#[sqlx::test]
async fn refuses_an_unassigned_slot(db: SqlitePool) {
    let app = TestApp::new(db);
    let heimdall = HeimdallBuilder::new().await;
    heimdall.assign(1, "Josef").await;
    heimdall.assign(2, "Ondra").await;
    let heimdall = heimdall.finish().await;

    let err = import_error(&app, &heimdall, &options("Josef")).await;

    assert!(
        matches!(err, ImportError::UnassignedSlot { id: 3, .. }),
        "{err}"
    );
    assert!(err.to_string().contains("emptyslot_3"), "{err}");
    assert_asgard_is_empty(&app).await;
}

#[sqlx::test]
async fn refuses_an_entry_by_an_unknown_user(db: SqlitePool) {
    let app = TestApp::new(db);
    let heimdall = brewing().await;
    heimdall.without_foreign_keys().await;
    heimdall
        .payment(2, 9, 50.0, "Komu?", "2025-10-21 08:00:00")
        .await;
    let heimdall = heimdall.finish().await;

    let err = import_error(&app, &heimdall, &options("Josef")).await;

    assert!(
        matches!(
            err,
            ImportError::UnknownUser {
                kind: EntryKind::Payment,
                id: 2,
                user: 9
            }
        ),
        "{err}"
    );
    assert_asgard_is_empty(&app).await;
}

#[sqlx::test]
async fn refuses_two_members_with_one_name_after_renames(db: SqlitePool) {
    let app = TestApp::new(db);
    let heimdall = brewing().await.finish().await;
    let options = ImportOptions {
        renames: vec![Rename {
            from: "Ondra".to_owned(),
            to: "josef".to_owned(),
        }],
        ..options("Josef")
    };

    let err = import_error(&app, &heimdall, &options).await;

    assert!(matches!(&err, ImportError::DuplicateName(_)), "{err}");
    assert_asgard_is_empty(&app).await;
}

#[sqlx::test]
async fn refuses_an_admin_name_that_matches_no_member(db: SqlitePool) {
    let app = TestApp::new(db);
    let heimdall = brewing().await.finish().await;

    let err = import_error(&app, &heimdall, &options("Thor")).await;

    assert!(
        matches!(&err, ImportError::UnknownAdmin(name) if name == "Thor"),
        "{err}"
    );
    assert_asgard_is_empty(&app).await;
}

#[sqlx::test]
async fn refuses_a_file_that_isnt_there(db: SqlitePool) {
    let app = TestApp::new(db);
    let missing = HeimdallBuilder::new().await.finish().await;
    let path = missing.path().with_file_name("missing.db");

    let err =
        crate::domain::heimdall::import(&app.state.db, &app.state.clock, &path, &options("Josef"))
            .await
            .unwrap_err();

    assert!(matches!(err, ImportError::Heimdall(_)), "{err}");
    assert!(!path.exists(), "the import created Heimdall's database");
}

#[sqlx::test]
async fn an_expense_is_dated_on_the_prague_day_it_was_recorded(db: SqlitePool) {
    let app = TestApp::new(db);
    let heimdall = HeimdallBuilder::crew().await;
    // 23:30 UTC on 14 November is 00:30 on 15 November in Prague.
    heimdall
        .expense(1, 30.0, "Pozdě večer", "2025-11-14 23:30:00")
        .await;
    heimdall
        .expense(1, 30.0, "Odpoledne", "2025-11-14 15:00:00")
        .await;
    let heimdall = heimdall.finish().await;

    run(&app, &heimdall, &options("Josef")).await.unwrap();

    let overview = app.state.finance.overview().await.unwrap();
    let dates: Vec<_> = overview
        .recent
        .iter()
        .map(|entry| (entry.amount().get(), entry.date().to_string()))
        .collect();
    assert_eq!(
        dates,
        [(30, "2025-11-15".to_owned()), (30, "2025-11-14".to_owned())]
    );
}

#[sqlx::test]
async fn every_member_shares_the_earliest_expense(db: SqlitePool) {
    let app = TestApp::new(db);
    let heimdall = HeimdallBuilder::crew().await;
    heimdall
        .expense(2, 600.0, "První várka", "2024-03-01 18:00:00")
        .await;
    heimdall
        .expense(1, 3.0, "Sáček", "2025-10-01 18:00:00")
        .await;
    let heimdall = heimdall.finish().await;

    run(&app, &heimdall, &options("Josef")).await.unwrap();

    assert_eq!(
        balances(&app).await,
        [
            ("Josef".to_owned(), -198),
            ("Ondra".to_owned(), 399),
            ("Pepa".to_owned(), -201)
        ]
    );
}

#[sqlx::test]
async fn rounds_amounts_to_whole_crowns_and_lists_them(db: SqlitePool) {
    let app = TestApp::new(db);
    let heimdall = HeimdallBuilder::crew().await;
    heimdall
        .expense(1, 299.5, "Slad", "2025-09-14 10:00:00")
        .await;
    heimdall
        .payment(2, 1, 99.4, "Vyrovnání", "2025-10-20 08:00:00")
        .await;
    let heimdall = heimdall.finish().await;

    let report = run(&app, &heimdall, &options("Josef"))
        .await
        .unwrap()
        .imported
        .report;

    assert!(report.passed(), "{report}");
    assert_eq!(
        report.rounded,
        [
            Rounding {
                kind: EntryKind::Expense,
                id: 1,
                original: 299.5,
                rounded: 300
            },
            Rounding {
                kind: EntryKind::Payment,
                id: 1,
                original: 99.4,
                rounded: 99
            }
        ]
    );
    assert!(
        report.to_string().contains("Expense 1: 299.5 -> 300"),
        "{report}"
    );
    let history = app
        .state
        .finance
        .history(&HistoryFilter::default(), 1)
        .await
        .unwrap();
    let mut amounts: Vec<_> = history.entries.iter().map(|e| e.amount().get()).collect();
    amounts.sort_unstable();
    assert_eq!(amounts, [99, 300]);
}

#[sqlx::test]
async fn a_balance_more_than_a_crown_off_rolls_back_and_fails(db: SqlitePool) {
    let app = TestApp::new(db);
    let heimdall = HeimdallBuilder::crew().await;
    // Heimdall sums Josef's thirty 1 Kč Expenses to 30 and splits that: 10 each. Asgard splits
    // each one on its own, and a crown can't be split three ways, so Josef keeps every one.
    for day in 1..=30 {
        heimdall
            .expense(1, 1.0, "Etiketa", &format!("2025-09-{day:02} 10:00:00"))
            .await;
    }
    let heimdall = heimdall.finish().await;

    let run = run(&app, &heimdall, &options("Josef")).await.unwrap();

    let report = &run.imported.report;
    assert!(!report.passed());
    assert!(!run.imported.committed);
    assert_eq!(
        report.failures(),
        [
            "Josef's Balance differs by -20.00 Kč, more than 1 Kč",
            "Ondra's Balance differs by 10.00 Kč, more than 1 Kč",
            "Pepa's Balance differs by 10.00 Kč, more than 1 Kč",
        ]
    );
    assert!(
        report
            .to_string()
            .contains("Verdict: FAIL. Nothing was committed:")
    );
    assert_eq!(run.device_links.len(), 0);
    assert_asgard_is_empty(&app).await;
}

#[sqlx::test]
async fn leaves_heimdalls_database_as_it_was(db: SqlitePool) {
    let app = TestApp::new(db);
    let heimdall = brewing().await.finish().await;
    let before = std::fs::read(heimdall.path()).unwrap();

    run(&app, &heimdall, &dry_run("Josef")).await.unwrap();
    run(&app, &heimdall, &options("Thor")).await.unwrap_err();
    run(&app, &heimdall, &options("Josef")).await.unwrap();

    assert_eq!(std::fs::read(heimdall.path()).unwrap(), before);
}

#[sqlx::test]
async fn reminders_still_to_come_are_pending_and_past_ones_are_sent(db: SqlitePool) {
    // The test app's clock reads 2026-01-01T00:00:00Z.
    let app = TestApp::new(db);
    let heimdall = HeimdallBuilder::crew().await;
    heimdall
        .reminder(
            2,
            "Stočit ležák",
            "2026-01-01T00:01:00+00:00",
            "2025-12-20 10:00:00",
        )
        .await;
    heimdall
        .reminder(
            3,
            "Objednat slad",
            "2025-12-31T23:59:00+00:00",
            "2025-12-20 11:00:00",
        )
        .await;
    let heimdall = heimdall.finish().await;

    let report = run(&app, &heimdall, &options("Josef"))
        .await
        .unwrap()
        .imported
        .report;

    assert!(
        report
            .counts
            .iter()
            .any(|row| row.kind == EntryKind::Reminder && row.heimdall == 2 && row.asgard == 2)
    );
    let pending = app.state.reminders.pending().await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].text.as_str(), "Stočit ležák");
    assert_eq!(pending[0].created_by.as_str(), "Ondra");
    assert_eq!(
        pending[0].due_at.crew_time().to_string(),
        "2026-01-01T01:01:00"
    );
    let sent = app.state.reminders.sent().await.unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].text.as_str(), "Objednat slad");
    assert_eq!(sent[0].created_by.as_str(), "Pepa");
    assert_eq!(
        sent[0].sent_at.crew_time().to_string(),
        "2026-01-01T00:59:00"
    );
    assert_eq!(
        app.state.reminders.send_due().await.unwrap(),
        0,
        "the ticker pushed a stale Reminder"
    );
}

#[sqlx::test]
async fn refuses_a_reminder_by_an_unknown_user(db: SqlitePool) {
    let app = TestApp::new(db);
    let heimdall = HeimdallBuilder::crew().await;
    heimdall.without_foreign_keys().await;
    heimdall
        .reminder(
            7,
            "Kdo?",
            "2026-02-01T09:00:00+00:00",
            "2025-12-20 10:00:00",
        )
        .await;
    let heimdall = heimdall.finish().await;

    let err = import_error(&app, &heimdall, &options("Josef")).await;

    assert!(
        matches!(
            err,
            ImportError::UnknownUser {
                kind: EntryKind::Reminder,
                id: 1,
                user: 7
            }
        ),
        "{err}"
    );
    assert_asgard_is_empty(&app).await;
}

#[sqlx::test]
async fn quotes_keep_their_text_author_and_time_and_are_searchable(db: SqlitePool) {
    let app = TestApp::new(db);
    let heimdall = HeimdallBuilder::crew().await;
    heimdall
        .quote(
            "Přišel jsem, uvařil jsem, vypil jsem.",
            "Ondra",
            "2025-06-01 18:30:00",
        )
        .await;
    heimdall
        .quote("Pivo je základ.", "Pepa", "2025-11-11 11:11:11")
        .await;
    let heimdall = heimdall.finish().await;

    let report = run(&app, &heimdall, &options("Josef"))
        .await
        .unwrap()
        .imported
        .report;

    assert!(
        report
            .counts
            .iter()
            .any(|row| row.kind == EntryKind::Quote && row.heimdall == 2 && row.asgard == 2)
    );
    let found = app
        .state
        .quotes
        .page(&Search::new("PRISEL"), 1)
        .await
        .unwrap();
    assert_eq!(found.quotes.len(), 1);
    let quote = &found.quotes[0];
    assert_eq!(quote.text.as_str(), "Přišel jsem, uvařil jsem, vypil jsem.");
    assert_eq!(quote.author.as_str(), "Ondra");
    assert_eq!(quote.added_by, None);
    assert_eq!(
        quote.added_at.crew_time().to_string(),
        "2025-06-01T20:30:00"
    );
    let by_author = app
        .state
        .quotes
        .page(&Search::new("pépa"), 1)
        .await
        .unwrap();
    assert_eq!(by_author.quotes.len(), 1);
}

#[sqlx::test]
async fn a_committed_import_prints_a_working_device_link_for_each_member(db: SqlitePool) {
    let app = TestApp::new(db);
    let heimdall = brewing().await.finish().await;

    let run = run(&app, &heimdall, &options("Josef")).await.unwrap();

    let labels: Vec<_> = run
        .device_links
        .iter()
        .map(|(name, _)| name.to_string())
        .collect();
    assert_eq!(labels, ["Josef", "Ondra", "Pepa"]);
    let open: Vec<_> = app
        .state
        .auth
        .open_links()
        .await
        .unwrap()
        .into_iter()
        .map(|link| link.display_name.to_string())
        .collect();
    assert_eq!(open, ["Josef", "Ondra", "Pepa"]);

    let (_, ondras_link) = &run.device_links[1];
    assert_eq!(
        ondras_link.origin(),
        Url::parse(PUBLIC_ORIGIN).unwrap().origin()
    );
    let response = app.join(ondras_link.path(), &mut authenticator()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = cookie_pair(&response);
    let settings = body_text(app.get_with("/settings", &cookie).await).await;
    assert!(settings.contains("Ondra"), "{settings}");
}

/// Signs in through `link`, the way a Member opens the Device link they were sent, and presses
/// "Got it" on the welcome page.
async fn sign_in_with(app: &TestApp, link: &Url) -> String {
    let response = app.join(link.path(), &mut authenticator()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = cookie_pair(&response);
    let got_it = app.post_with("/welcome/done", &cookie).await;
    assert_eq!(got_it.status(), StatusCode::SEE_OTHER);
    cookie
}

async fn page(app: &TestApp, uri: &str, cookie: &str) -> String {
    let response = app.get_with(uri, cookie).await;
    assert_eq!(response.status(), StatusCode::OK, "{uri}");
    body_text(response).await
}

#[sqlx::test]
async fn every_tab_shows_a_full_import(db: SqlitePool) {
    let app = TestApp::new(db);
    let heimdall = brewing().await;
    heimdall
        .reminder(
            2,
            "Stočit ležák",
            "2026-01-05T17:00:00+00:00",
            "2025-12-20 10:00:00",
        )
        .await;
    heimdall
        .reminder(
            3,
            "Objednat slad",
            "2025-12-30T09:00:00+00:00",
            "2025-12-20 11:00:00",
        )
        .await;
    heimdall
        .quote("Pivo je základ.", "Pepa", "2025-11-11 11:11:11")
        .await;
    let heimdall = heimdall.finish().await;
    let run = run(&app, &heimdall, &options("Josef")).await.unwrap();
    assert!(run.imported.committed, "{}", run.imported.report);
    let cookie = sign_in_with(&app, &run.device_links[0].1).await;

    let finance = page(&app, "/", &cookie).await;
    assert!(finance.contains("Slad a chmel"), "{finance}");
    assert!(finance.contains("Vyrovnání"), "{finance}");
    let history = page(&app, "/history", &cookie).await;
    assert!(history.contains("Kvasnice"), "{history}");
    let reminders = page(&app, "/reminders", &cookie).await;
    assert!(reminders.contains("Stočit ležák"), "{reminders}");
    assert!(reminders.contains("Objednat slad"), "{reminders}");
    let quotes = page(&app, "/quotes", &cookie).await;
    assert!(quotes.contains("Pivo je základ."), "{quotes}");
}
