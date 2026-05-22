CREATE TABLE IF NOT EXISTS minecraft_passwords (
    username   TEXT PRIMARY KEY,
    hash       TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
