-- Whether a Member has pressed "Got it" on the welcome page, which then never shows again.

ALTER TABLE members ADD COLUMN welcomed BOOLEAN NOT NULL DEFAULT FALSE;
