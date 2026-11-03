-- Heimdall's schema as it stands: its migrations, in order, copied verbatim from the Heimdall
-- repo (josefjura/heimdall @ 89f2148). The import tests build Heimdall databases from it.

-- 20260120092507_expense.sql
CREATE TABLE expenses (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    description TEXT NOT NULL,
    owner_id INTEGER NOT NULL, -- Odkaz na users.id
    amount REAL NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (owner_id) REFERENCES users(id)
);

-- 20260120092841_payment.sql
CREATE TABLE payments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    owner_id INTEGER NOT NULL,      -- Kdo platí
		description TEXT NOT NULL,
    recipient_id INTEGER NOT NULL,  -- Komu platí
    amount REAL NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (owner_id) REFERENCES users(id),
    FOREIGN KEY (recipient_id) REFERENCES users(id)
);

-- 20260120094415_quotes.sql
-- Add migration script here
CREATE TABLE IF NOT EXISTS quotes (
		id INTEGER PRIMARY KEY AUTOINCREMENT,
		quote_text TEXT NOT NULL,
		author_name TEXT NOT NULL,
		created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 20260120160302_users.sql
-- Add migration script here
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    discord_id TEXT UNIQUE NOT NULL,
    display_name TEXT NOT NULL
);

INSERT INTO users (id, discord_id, display_name) VALUES ('1','emptyslot_1', 'Viking 1');
INSERT INTO users (id, discord_id, display_name) VALUES ('2','emptyslot_2', 'Viking 2');
INSERT INTO users (id, discord_id, display_name) VALUES ('3','emptyslot_3', 'Viking 3');

-- 20260123134415_reminders.sql
-- Add migration script here
CREATE TABLE IF NOT EXISTS reminders (
		id INTEGER PRIMARY KEY AUTOINCREMENT,
		text TEXT NOT NULL,
		date TIMESTAMP NOT NULL,
		created_by INTEGER NOT NULL, -- Odkaz na users.id		
		created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 20260201000001_add_indexes_users.sql
-- Add index on discord_id for faster user lookups
-- This column is used in every Discord command to find users
CREATE INDEX IF NOT EXISTS idx_users_discord_id ON users(discord_id);


-- 20260201000002_add_indexes_expenses.sql
-- Add indexes for expense queries
-- owner_id is used for filtering expenses by user
CREATE INDEX IF NOT EXISTS idx_expenses_owner_id ON expenses(owner_id);

-- created_at DESC is used for sorting expenses chronologically
CREATE INDEX IF NOT EXISTS idx_expenses_created_at ON expenses(created_at DESC);

-- Composite index for efficient pagination (sorted by created_at, then id)
CREATE INDEX IF NOT EXISTS idx_expenses_created_at_id ON expenses(created_at DESC, id DESC);


-- 20260201000003_add_indexes_payments.sql
-- Add indexes for payment queries
-- owner_id and recipient_id are used for filtering payments by users
CREATE INDEX IF NOT EXISTS idx_payments_owner_id ON payments(owner_id);
CREATE INDEX IF NOT EXISTS idx_payments_recipient_id ON payments(recipient_id);

-- created_at DESC is used for sorting payments chronologically
CREATE INDEX IF NOT EXISTS idx_payments_created_at ON payments(created_at DESC);

-- Composite index for efficient pagination (sorted by created_at, then id)
CREATE INDEX IF NOT EXISTS idx_payments_created_at_id ON payments(created_at DESC, id DESC);


-- 20260201000004_add_indexes_reminders.sql
-- Add indexes for reminder queries
-- date is critical for reminder_ticker to find due reminders
CREATE INDEX IF NOT EXISTS idx_reminders_date ON reminders(date);

-- created_by is used for filtering reminders by user
CREATE INDEX IF NOT EXISTS idx_reminders_created_by ON reminders(created_by);

