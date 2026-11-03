//! Heimdall's database, read once and read-only. The import knows Heimdall's schema as it stands
//! (`users`, `expenses`, `payments`, `reminders`, `quotes`) and nothing more.

use std::path::Path;

use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Connection};

/// A row of Heimdall's `users`: a Discord-bound slot, or one still waiting for its person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub id: i64,
    pub discord_id: String,
    pub display_name: String,
}

impl User {
    /// Whether nobody has claimed the slot yet: Heimdall seeds its users as `emptyslot_*` rows.
    #[must_use]
    pub fn is_unassigned(&self) -> bool {
        self.discord_id.starts_with("emptyslot_")
    }
}

/// A row of Heimdall's `expenses`. The owner is the Payer.
#[derive(Debug, Clone, PartialEq)]
pub struct Expense {
    pub id: i64,
    pub owner_id: i64,
    pub amount: f64,
    pub description: String,
    pub created_at: Option<String>,
}

/// A row of Heimdall's `payments`. The owner is the Sender.
#[derive(Debug, Clone, PartialEq)]
pub struct Payment {
    pub id: i64,
    pub owner_id: i64,
    pub recipient_id: i64,
    pub amount: f64,
    pub description: String,
    pub created_at: Option<String>,
}

/// A row of Heimdall's `reminders`. `date` is when it is due.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reminder {
    pub id: i64,
    pub text: String,
    pub date: String,
    pub created_by: i64,
    pub created_at: Option<String>,
}

/// A row of Heimdall's `quotes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quote {
    pub id: i64,
    pub text: String,
    pub author: String,
    pub created_at: Option<String>,
}

/// Everything the import reads from Heimdall, each table in id order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Heimdall {
    pub users: Vec<User>,
    pub expenses: Vec<Expense>,
    pub payments: Vec<Payment>,
    pub reminders: Vec<Reminder>,
    pub quotes: Vec<Quote>,
}

/// Reads Heimdall's database at `path`, opened read-only so a failed import can't touch it.
///
/// Amounts are read as floats and instants as the text Heimdall stored, whatever type SQLite
/// kept them as.
///
/// # Errors
///
/// Fails if the file is missing, isn't a Heimdall database, or can't be read.
pub async fn read(path: &Path) -> Result<Heimdall, sqlx::Error> {
    let mut conn = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .connect()
        .await?;

    let users = sqlx::query_as::<_, (i64, String, String)>(
        "SELECT id, discord_id, display_name FROM users ORDER BY id",
    )
    .fetch_all(&mut conn)
    .await?
    .into_iter()
    .map(|(id, discord_id, display_name)| User {
        id,
        discord_id,
        display_name,
    })
    .collect();

    let expenses = sqlx::query_as::<_, (i64, i64, f64, String, Option<String>)>(
        "SELECT id, owner_id, CAST(amount AS REAL), description, CAST(created_at AS TEXT)
         FROM expenses ORDER BY id",
    )
    .fetch_all(&mut conn)
    .await?
    .into_iter()
    .map(|(id, owner_id, amount, description, created_at)| Expense {
        id,
        owner_id,
        amount,
        description,
        created_at,
    })
    .collect();

    let payments = sqlx::query_as::<_, (i64, i64, i64, f64, String, Option<String>)>(
        "SELECT id, owner_id, recipient_id, CAST(amount AS REAL), description,
                CAST(created_at AS TEXT)
         FROM payments ORDER BY id",
    )
    .fetch_all(&mut conn)
    .await?
    .into_iter()
    .map(
        |(id, owner_id, recipient_id, amount, description, created_at)| Payment {
            id,
            owner_id,
            recipient_id,
            amount,
            description,
            created_at,
        },
    )
    .collect();

    let reminders = sqlx::query_as::<_, (i64, String, String, i64, Option<String>)>(
        "SELECT id, text, CAST(date AS TEXT), created_by, CAST(created_at AS TEXT)
         FROM reminders ORDER BY id",
    )
    .fetch_all(&mut conn)
    .await?
    .into_iter()
    .map(|(id, text, date, created_by, created_at)| Reminder {
        id,
        text,
        date,
        created_by,
        created_at,
    })
    .collect();

    let quotes = sqlx::query_as::<_, (i64, String, String, Option<String>)>(
        "SELECT id, quote_text, author_name, CAST(created_at AS TEXT) FROM quotes ORDER BY id",
    )
    .fetch_all(&mut conn)
    .await?
    .into_iter()
    .map(|(id, text, author, created_at)| Quote {
        id,
        text,
        author,
        created_at,
    })
    .collect();

    conn.close().await?;
    Ok(Heimdall {
        users,
        expenses,
        payments,
        reminders,
        quotes,
    })
}
