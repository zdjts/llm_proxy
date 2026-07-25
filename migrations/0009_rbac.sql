-- 0009: RBAC (Role-Based Access Control) tables (T134, T141-T144)
-- Part of v3.0 full-stack upgrade per docs/BRIEF-v3.0-fullstack-upgrade.md

CREATE TABLE IF NOT EXISTS user_account (
    id          TEXT PRIMARY KEY,                  -- UUID
    email       TEXT NOT NULL UNIQUE,
    name        TEXT NOT NULL,
    password_hash TEXT,                             -- nullable for SSO-only users
    sso_subject TEXT,                               -- OIDC/SAML subject claim
    avatar_url  TEXT,
    disabled    INTEGER NOT NULL DEFAULT 0,
    last_login  INTEGER,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000),
    updated_at  INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000)
);

CREATE TABLE IF NOT EXISTS team (
    id          TEXT PRIMARY KEY,                  -- UUID
    name        TEXT NOT NULL,
    slug        TEXT NOT NULL UNIQUE,
    description TEXT,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000),
    updated_at  INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000)
);

CREATE TABLE IF NOT EXISTS team_member (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    team_id     TEXT NOT NULL REFERENCES team(id),
    user_id     TEXT NOT NULL REFERENCES user_account(id),
    role_in_team TEXT NOT NULL DEFAULT 'viewer',    -- 'owner' | 'admin' | 'member' | 'viewer' (per-team role, separate from RBAC)
    joined_at   INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000),
    UNIQUE(team_id, user_id)
);

CREATE TABLE IF NOT EXISTS rbac_role (
    id          TEXT PRIMARY KEY,                  -- e.g. "owner", "admin", "operator", "billing", "readonly", "portal_user"
    name        TEXT NOT NULL,
    description TEXT,
    is_system   INTEGER NOT NULL DEFAULT 0,        -- 1 = built-in, cannot be deleted
    created_at  INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000)
);

CREATE TABLE IF NOT EXISTS rbac_permission (
    id          TEXT PRIMARY KEY,                  -- e.g. "providers.manage", "keys.rotate", "routing.edit"
    description TEXT,
    category    TEXT NOT NULL DEFAULT 'general'     -- groupings: providers, keys, routing, pipeline, quotas, billing, alerts, team, audit, portal
);

CREATE TABLE IF NOT EXISTS rbac_role_permission (
    role_id         TEXT NOT NULL REFERENCES rbac_role(id),
    permission_id   TEXT NOT NULL REFERENCES rbac_permission(id),
    PRIMARY KEY (role_id, permission_id)
);

-- User-to-role assignment (global, not per-team — per-team roles are in team_member.role_in_team)
CREATE TABLE IF NOT EXISTS user_role (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id     TEXT NOT NULL REFERENCES user_account(id),
    role_id     TEXT NOT NULL REFERENCES rbac_role(id),
    UNIQUE(user_id, role_id)
);

-- API tokens (JWT refresh tokens stored for revocation check)
CREATE TABLE IF NOT EXISTS refresh_token (
    id          TEXT PRIMARY KEY,                  -- token jti (JWT ID)
    user_id     TEXT NOT NULL REFERENCES user_account(id),
    expires_at  INTEGER NOT NULL,
    revoked     INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch('subsec') * 1000)
);

CREATE INDEX IF NOT EXISTS idx_user_account_email ON user_account(email);
CREATE INDEX IF NOT EXISTS idx_team_member_user ON team_member(user_id);
CREATE INDEX IF NOT EXISTS idx_team_member_team ON team_member(team_id);
CREATE INDEX IF NOT EXISTS idx_user_role_user ON user_role(user_id);
CREATE INDEX IF NOT EXISTS idx_refresh_token_user ON refresh_token(user_id);
