//! The one-off import of Heimdall's data on the cutover day (ADR 0002).
//!
//! Members, Expenses, Payments, Reminders and Quotes are written in one transaction through
//! the services' import paths, then checked against Heimdall's own figures before anything is
//! committed.

mod balances;
mod plan;
mod report;
mod source;

use std::collections::BTreeMap;
use std::path::Path;

use sqlx::{SqliteConnection, SqlitePool};
use webauthn_rs::prelude::Uuid;

use self::balances::heimdall_balances;
pub use self::plan::{EntryKind, ImportError, ImportOptions, Rename, Rounding};
use self::plan::{Plan, whole_crowns};
pub use self::report::{BalanceRow, CountRow, Report, TotalRow};
use self::source::Heimdall;
use crate::domain::clock::{Clock, unix_seconds};
use crate::domain::finance::{self, ImportedExpense, ImportedPayment};
use crate::domain::members::{self, MemberError, MemberId, MemberName, NameError, NewMember};
use crate::domain::quotes;
use crate::domain::reminders::{self, ImportedReminder};

/// What an import did.
#[derive(Debug, Clone)]
pub struct Imported {
    pub report: Report,
    /// Whether it was committed: it passed and wasn't a dry run.
    pub committed: bool,
    /// The Members it created, in Heimdall's order. Gone again unless it was committed.
    pub members: Vec<MemberName>,
    /// The Admin among them.
    pub admin: MemberId,
}

/// Imports the Heimdall database at `heimdall` into the empty Asgard database `asgard`, as
/// `options` say, at the time `clock` shows.
///
/// Everything is written in one transaction and verified before it is committed: the import
/// commits only if it [passed](Report::passed) and isn't a dry run. Otherwise it rolls back and
/// leaves Asgard's database as empty as it was.
///
/// # Errors
///
/// Returns an [`ImportError`], with nothing written, if Heimdall's database can't be read,
/// Asgard's isn't empty, or Heimdall's data can't be mapped onto Asgard's.
pub async fn import(
    asgard: &SqlitePool,
    clock: &Clock,
    heimdall: &Path,
    options: &ImportOptions,
) -> Result<Imported, ImportError> {
    let source = source::read(heimdall)
        .await
        .map_err(ImportError::Heimdall)?;
    let plan = Plan::new(&source, options, clock.today())?;
    let now = unix_seconds(clock.now());

    let mut tx = asgard.begin().await?;
    if !is_empty(&mut tx).await? {
        return Err(ImportError::NotEmpty);
    }
    let ids = write(&mut tx, &plan, now).await?;
    let report = verify(&mut tx, &source, &plan, &ids, options.dry_run).await?;
    let committed = report.passed() && !options.dry_run;
    if committed {
        tx.commit().await?;
    } else {
        tx.rollback().await?;
    }

    let members = plan
        .members
        .iter()
        .map(|member| MemberName {
            id: ids[&member.heimdall_id],
            display_name: member.display_name.clone(),
        })
        .collect();
    let admin = plan
        .members
        .iter()
        .find(|member| member.is_admin)
        .map(|member| ids[&member.heimdall_id])
        .ok_or_else(|| ImportError::UnknownAdmin(options.admin.clone()))?;
    Ok(Imported {
        report,
        committed,
        members,
        admin,
    })
}

/// Whether Asgard has no Members and nothing anyone entered.
async fn is_empty(conn: &mut SqliteConnection) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT NOT EXISTS (SELECT 1 FROM members)
              AND NOT EXISTS (SELECT 1 FROM expenses)
              AND NOT EXISTS (SELECT 1 FROM payments)
              AND NOT EXISTS (SELECT 1 FROM reminders)
              AND NOT EXISTS (SELECT 1 FROM items)
              AND NOT EXISTS (SELECT 1 FROM quotes) AS "empty!: bool""#
    )
    .fetch_one(conn)
    .await
}

/// Writes everything `plan` holds, and answers which Member each Heimdall user became.
async fn write(
    conn: &mut SqliteConnection,
    plan: &Plan,
    now: i64,
) -> Result<BTreeMap<i64, MemberId>, ImportError> {
    let mut ids = BTreeMap::new();
    for member in &plan.members {
        let id = members::insert(
            conn,
            NewMember {
                display_name: &member.display_name,
                is_admin: member.is_admin,
                joined_on: plan.joined_on,
                webauthn_user_id: &Uuid::new_v4().to_string(),
            },
        )
        .await
        .map_err(|err| match err {
            MemberError::Name(NameError::Taken(name)) => ImportError::DuplicateName(name),
            MemberError::Name(err) => ImportError::InvalidName {
                heimdall_name: member.display_name.to_string(),
                err,
            },
            MemberError::Database(err) => ImportError::Database(err),
        })?;
        ids.insert(member.heimdall_id, id);
    }

    for expense in &plan.expenses {
        finance::import_expense(
            conn,
            &ImportedExpense {
                payer: ids[&expense.payer],
                amount: expense.amount,
                description: expense.description.clone(),
                expense_date: expense.expense_date,
                logged_at: expense.logged_at,
            },
        )
        .await?;
    }
    for payment in &plan.payments {
        finance::import_payment(
            conn,
            &ImportedPayment {
                sender: ids[&payment.sender],
                recipient: ids[&payment.recipient],
                amount: payment.amount,
                description: payment.description.clone(),
                logged_at: payment.logged_at,
            },
        )
        .await?;
    }
    for reminder in &plan.reminders {
        reminders::import(
            conn,
            &ImportedReminder {
                text: reminder.text.clone(),
                created_by: ids[&reminder.created_by],
                due_at: reminder.due_at,
                created_at: reminder.created_at,
            },
            now,
        )
        .await?;
    }
    for quote in &plan.quotes {
        quotes::import(conn, &quote.quote, quote.added_at).await?;
    }
    Ok(ids)
}

/// Compares what Asgard now holds, read back through the services, with Heimdall's own figures.
async fn verify(
    conn: &mut SqliteConnection,
    heimdall: &Heimdall,
    plan: &Plan,
    ids: &BTreeMap<i64, MemberId>,
    dry_run: bool,
) -> Result<Report, sqlx::Error> {
    let count = |rows: usize| i64::try_from(rows).unwrap_or(i64::MAX);
    let totals = finance::totals(conn).await?;
    let counts = vec![
        CountRow {
            kind: EntryKind::Expense,
            heimdall: count(heimdall.expenses.len()),
            asgard: totals.expense_count,
        },
        CountRow {
            kind: EntryKind::Payment,
            heimdall: count(heimdall.payments.len()),
            asgard: totals.payment_count,
        },
        CountRow {
            kind: EntryKind::Reminder,
            heimdall: count(heimdall.reminders.len()),
            asgard: reminders::count(conn).await?,
        },
        CountRow {
            kind: EntryKind::Quote,
            heimdall: count(heimdall.quotes.len()),
            asgard: quotes::count(conn).await?,
        },
    ];
    let money = vec![
        TotalRow {
            kind: EntryKind::Expense,
            heimdall: plan.expenses.iter().map(|e| e.amount.get()).sum(),
            asgard: totals.expense_total,
        },
        TotalRow {
            kind: EntryKind::Payment,
            heimdall: plan.payments.iter().map(|p| p.amount.get()).sum(),
            asgard: totals.payment_total,
        },
    ];

    let heimdall_ids: BTreeMap<MemberId, i64> = ids.iter().map(|(&h, &m)| (m, h)).collect();
    let in_heimdall = heimdall_balances(heimdall);
    let balances = finance::balances(conn)
        .await?
        .into_iter()
        .map(|row| {
            let heimdall = heimdall_ids
                .get(&row.member.id)
                .and_then(|id| in_heimdall.get(id))
                .copied()
                .unwrap_or_default();
            BalanceRow {
                member: row.member.display_name,
                // Every amount was finite when it was planned, so the Balance is too.
                heimdall_hundredths: whole_crowns(heimdall * 100.0).unwrap_or_default(),
                asgard: row.balance,
            }
        })
        .collect();

    Ok(Report {
        counts,
        totals: money,
        balances,
        rounded: plan.rounded.clone(),
        dry_run,
    })
}

/// Heimdall databases for tests, built from Heimdall's real schema.
#[cfg(test)]
pub(crate) mod fixtures {
    use std::path::{Path, PathBuf};

    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use sqlx::{Executor, SqlitePool};
    use tempfile::TempDir;

    /// Heimdall's migrations, in order, as its repo has them.
    const SCHEMA: &str = include_str!("heimdall_schema.sql");

    /// A Heimdall database being filled for a scenario. Its migrations seed three unassigned
    /// slots, as a fresh Heimdall has.
    pub struct HeimdallBuilder {
        dir: TempDir,
        pool: SqlitePool,
    }

    /// A finished Heimdall database file, kept until this is dropped.
    pub struct HeimdallFile {
        _dir: TempDir,
        path: PathBuf,
    }

    impl HeimdallFile {
        pub fn path(&self) -> &Path {
            &self.path
        }
    }

    impl HeimdallBuilder {
        pub async fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let options = SqliteConnectOptions::new()
                .filename(dir.path().join("heimdall.db"))
                .create_if_missing(true);
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(options)
                .await
                .unwrap();
            pool.execute(sqlx::raw_sql(SCHEMA)).await.unwrap();
            Self { dir, pool }
        }

        /// Heimdall with its three slots claimed by Josef, Ondra and Pepa (users 1, 2 and 3).
        pub async fn crew() -> Self {
            let heimdall = Self::new().await;
            for (id, name) in [(1, "Josef"), (2, "Ondra"), (3, "Pepa")] {
                heimdall.assign(id, name).await;
            }
            heimdall
        }

        /// Gives the slot `id` to a Discord user shown as `name`.
        pub async fn assign(&self, id: i64, name: &str) {
            sqlx::query("UPDATE users SET discord_id = ?, display_name = ? WHERE id = ?")
                .bind(format!("10000000000000000{id}"))
                .bind(name)
                .bind(id)
                .execute(&self.pool)
                .await
                .unwrap();
        }

        /// Lets rows refer to users that don't exist, as nothing in Heimdall's schema stops.
        pub async fn without_foreign_keys(&self) {
            sqlx::query("PRAGMA foreign_keys = OFF")
                .execute(&self.pool)
                .await
                .unwrap();
        }

        /// An Expense paid by user `owner`, recorded at `created_at` (UTC, as Heimdall's
        /// `CURRENT_TIMESTAMP` writes it).
        pub async fn expense(&self, owner: i64, amount: f64, description: &str, created_at: &str) {
            sqlx::query(
                "INSERT INTO expenses (owner_id, amount, description, created_at) VALUES (?, ?, ?, ?)",
            )
            .bind(owner)
            .bind(amount)
            .bind(description)
            .bind(created_at)
            .execute(&self.pool)
            .await
            .unwrap();
        }

        /// A Payment from user `owner` to user `recipient`.
        pub async fn payment(
            &self,
            owner: i64,
            recipient: i64,
            amount: f64,
            description: &str,
            created_at: &str,
        ) {
            sqlx::query(
                "INSERT INTO payments (owner_id, recipient_id, amount, description, created_at)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(owner)
            .bind(recipient)
            .bind(amount)
            .bind(description)
            .bind(created_at)
            .execute(&self.pool)
            .await
            .unwrap();
        }

        /// A Reminder due at `date`, which Heimdall's bot stores with an offset
        /// (`2026-11-07T09:00:00+00:00`).
        pub async fn reminder(&self, created_by: i64, text: &str, date: &str, created_at: &str) {
            sqlx::query(
                "INSERT INTO reminders (created_by, text, date, created_at) VALUES (?, ?, ?, ?)",
            )
            .bind(created_by)
            .bind(text)
            .bind(date)
            .bind(created_at)
            .execute(&self.pool)
            .await
            .unwrap();
        }

        pub async fn quote(&self, text: &str, author: &str, created_at: &str) {
            sqlx::query(
                "INSERT INTO quotes (quote_text, author_name, created_at) VALUES (?, ?, ?)",
            )
            .bind(text)
            .bind(author)
            .bind(created_at)
            .execute(&self.pool)
            .await
            .unwrap();
        }

        /// Closes the database, so the import reads it as Heimdall left it.
        pub async fn finish(self) -> HeimdallFile {
            self.pool.close().await;
            let path = self.dir.path().join("heimdall.db");
            HeimdallFile {
                _dir: self.dir,
                path,
            }
        }
    }
}
