# mattermine

A Mattermost bot that manages Minecraft login passwords for corporate LDAP users.

Users send commands via **direct message** to the bot. Passwords are hashed with
Argon2id before storage — plaintext is never persisted. The `/set-password` message
is deleted from chat immediately after processing to keep credentials out of logs.

## Commands

| Command | Description |
|---|---|
| `/set-password <password>` | Create or update your Minecraft password |
| `/my-password` | Check whether a password is set (no plaintext shown) |

## How usernames work

Each user's Mattermost handle (without `@`) is their LDAP username, used as the
primary key in the database. The bot resolves handles automatically from the
Mattermost API.

## Rate limiting

After 5 consecutive failed `/set-password` attempts, further attempts are blocked
for 10 minutes. Rate-limit state is in-memory and resets on bot restart.

## Configuration

Set these environment variables (copy `.env.example`):

| Variable | Description |
|---|---|
| `MATTERMOST_URL` | Full URL of your Mattermost instance (e.g. `https://chat.example.com`) |
| `MATTERMOST_TOKEN` | Bot account access token |
| `DATABASE_URL` | PostgreSQL connection string |
| `RUST_LOG` | Log level, e.g. `mattermine=info` (optional) |

## Running with Docker Compose

```bash
cp .env.example .env
# Edit .env with real values

docker compose up --build
```

The compose file starts a PostgreSQL 16 instance alongside the bot. The database
schema is applied automatically on first start.

## Running locally

Requirements: Rust 1.78+, a running PostgreSQL instance.

```bash
cp .env.example .env
# Edit .env

export $(grep -v '^#' .env | xargs)
cargo run
```

## Database

The bot uses a single table:

```sql
CREATE TABLE minecraft_passwords (
    username   TEXT PRIMARY KEY,
    hash       TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

The bot only needs `INSERT`, `UPDATE`, and `SELECT` rights on this table. It does
not require admin database credentials.

Grant minimal privileges:

```sql
CREATE USER mattermine_bot WITH PASSWORD 'strong-password';
GRANT SELECT, INSERT, UPDATE ON minecraft_passwords TO mattermine_bot;
```

## Creating a Mattermost bot account

1. In Mattermost: **System Console → Integrations → Bot Accounts → Add Bot Account**
2. Give it a username (e.g. `mattermine`)
3. Copy the generated token into `MATTERMOST_TOKEN`
4. Ensure the bot account can receive direct messages from users

## Security notes

- Passwords are hashed with Argon2id (PHC string format) — the hash cannot be
  reversed to recover the plaintext
- `/set-password` messages are deleted via the Mattermost API immediately after
  processing
- The bot token should belong to a dedicated bot account with minimal permissions
