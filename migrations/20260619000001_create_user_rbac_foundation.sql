-- ============================================================================
-- KOKKAK NEW DB Schema V.2 - User & RBAC Foundation (M14-prep)
--
-- Reference: kokkeak/NEW_DB.txt (DBML, 67 tables) + KOKKAK_MIGRATION_PLAN/02_DATABASE.md
--
-- This migration creates the 4 RBAC tables required by the Rust `User` aggregate:
--   [user]            - profile (first_name, last_name, tel, status, geo flags)
--   [user_username]   - login + password hash (1:1 to user)
--   [user_role]       - role catalog (seeded by 20260619000002)
--   [user_user_role]  - M:N user <-> role + department/scope metadata
--
-- Idempotent: every CREATE wrapped in IF OBJECT_ID check; every index in
-- IF NOT EXISTS (sys.indexes). Safe to re-run.
--
-- ponytail: the schema mirrors NEW_DB.txt 1:1 (snake_case columns prefixed
-- with table name). Optional fields (id_card, province_id, ...) are NULL
-- until the profile endpoints land. Ceiling: when chat/notification/
-- payment tables arrive in M15+ they reuse the same conventions.
-- ============================================================================

-- 1. [user] - main profile table
IF OBJECT_ID('[user]', 'U') IS NULL
CREATE TABLE [user] (
    user_guid                UNIQUEIDENTIFIER NOT NULL PRIMARY KEY,
    user_first_name          NVARCHAR(100)    NOT NULL,
    user_last_name           NVARCHAR(100)    NOT NULL,
    user_id_card             NVARCHAR(32)     NULL,
    user_tel                 NVARCHAR(32)     NULL,
    user_is_foreign          BIT              NOT NULL DEFAULT 0,
    user_stage_id            INT              NOT NULL DEFAULT 0,
    user_province_id         INT              NULL,
    user_district_id         INT              NULL,
    user_sub_district_id     INT              NULL,
    user_village_id          INT              NULL,
    user_post                NVARCHAR(16)     NULL,
    user_is_customer_company BIT              NOT NULL DEFAULT 0,
    user_is_customer         BIT              NOT NULL DEFAULT 0,
    user_is_employee         BIT              NOT NULL DEFAULT 0,
    user_is_freelance        BIT              NOT NULL DEFAULT 0,
    -- Status convention: 0=Pending 1=Active 2=Suspended 3=Deleted
    user_status              INT              NOT NULL DEFAULT 1,
    user_description         NVARCHAR(MAX)    NULL,
    user_create_at           DATETIME2(7)     NOT NULL DEFAULT SYSUTCDATETIME(),
    user_create_by           UNIQUEIDENTIFIER NULL,
    user_update_at           DATETIME2(7)     NOT NULL DEFAULT SYSUTCDATETIME(),
    user_update_by           UNIQUEIDENTIFIER NULL
);
GO

-- 2. [user_username] - login + password (separated for security per NEW_DB.txt)
IF OBJECT_ID('[user_username]', 'U') IS NULL
CREATE TABLE [user_username] (
    user_username_guid        UNIQUEIDENTIFIER NOT NULL PRIMARY KEY,
    user_username_user_guid   UNIQUEIDENTIFIER NOT NULL REFERENCES [user](user_guid),
    user_username_username    NVARCHAR(255)    NOT NULL,
    -- argon2id PHC string. Never log (AGENTS.md §12.1).
    user_username_password    NVARCHAR(512)    NOT NULL,
    user_username_status      INT              NOT NULL DEFAULT 1,
    user_username_remark      NVARCHAR(MAX)    NULL,
    user_username_create_at   DATETIME2(7)     NOT NULL DEFAULT SYSUTCDATETIME(),
    user_username_create_by   UNIQUEIDENTIFIER NULL,
    user_username_update_at   DATETIME2(7)     NOT NULL DEFAULT SYSUTCDATETIME(),
    user_username_update_by   UNIQUEIDENTIFIER NULL,
    CONSTRAINT uq_user_username_username UNIQUE (user_username_username)
);
GO

IF NOT EXISTS (SELECT 1 FROM sys.indexes WHERE name = 'ix_user_username_user_guid')
CREATE INDEX ix_user_username_user_guid ON [user_username](user_username_user_guid);
GO

-- 3. [user_role] - role catalog (seeded by 20260619000002)
IF OBJECT_ID('[user_role]', 'U') IS NULL
CREATE TABLE [user_role] (
    user_role_guid            UNIQUEIDENTIFIER NOT NULL PRIMARY KEY,
    user_role_code            NVARCHAR(32)     NOT NULL,
    user_role_name            NVARCHAR(100)    NOT NULL,
    user_role_description     NVARCHAR(MAX)    NULL,
    -- user_role_is_master: true for system roles (Super Admin) that cannot be deleted
    user_role_is_master       BIT              NOT NULL DEFAULT 0,
    -- data_scope_type: all / department / own / custom (per PLAN §12.3 RBAC)
    user_role_data_scope_type NVARCHAR(32)     NULL,
    user_role_status          INT              NOT NULL DEFAULT 1,
    user_role_create_at       DATETIME2(7)     NOT NULL DEFAULT SYSUTCDATETIME(),
    user_role_create_by       UNIQUEIDENTIFIER NULL,
    user_role_update_at       DATETIME2(7)     NOT NULL DEFAULT SYSUTCDATETIME(),
    user_role_update_by       UNIQUEIDENTIFIER NULL,
    CONSTRAINT uq_user_role_code UNIQUE (user_role_code),
    CONSTRAINT uq_user_role_name UNIQUE (user_role_name)
);
GO

-- 4. [user_user_role] - M:N junction with department + scope metadata
IF OBJECT_ID('[user_user_role]', 'U') IS NULL
CREATE TABLE [user_user_role] (
    user_user_role_guid                UNIQUEIDENTIFIER NOT NULL PRIMARY KEY,
    user_user_role_user_guid           UNIQUEIDENTIFIER NOT NULL REFERENCES [user](user_guid),
    user_user_role_role_guid           UNIQUEIDENTIFIER NOT NULL REFERENCES [user_role](user_role_guid),
    -- Optional department binding (NEW_DB.txt; full department tables land later)
    user_user_role_department_guid     UNIQUEIDENTIFIER NULL,
    user_user_role_department_parent_guid UNIQUEIDENTIFIER NULL,
    -- JSON config for data scope (e.g. {"provinces":[10,20], "tier":1})
    user_user_role_data_scope_config   NVARCHAR(MAX)    NULL,
    user_user_role_assigned_by         UNIQUEIDENTIFIER NULL REFERENCES [user](user_guid),
    user_user_role_assigned_at         DATETIME2(7)     NULL,
    user_user_role_expire_at           DATETIME2(7)     NULL,
    user_user_role_status              INT              NOT NULL DEFAULT 1,
    user_user_role_create_at           DATETIME2(7)     NOT NULL DEFAULT SYSUTCDATETIME(),
    user_user_role_create_by           UNIQUEIDENTIFIER NULL,
    user_user_role_update_at           DATETIME2(7)     NOT NULL DEFAULT SYSUTCDATETIME(),
    user_user_role_update_by           UNIQUEIDENTIFIER NULL,
    CONSTRAINT uq_user_role_assignment UNIQUE (user_user_role_user_guid, user_user_role_role_guid)
);
GO

IF NOT EXISTS (SELECT 1 FROM sys.indexes WHERE name = 'ix_user_user_role_user_guid')
CREATE INDEX ix_user_user_role_user_guid ON [user_user_role](user_user_role_user_guid);
GO

IF NOT EXISTS (SELECT 1 FROM sys.indexes WHERE name = 'ix_user_user_role_role_guid')
CREATE INDEX ix_user_user_role_role_guid ON [user_user_role](user_user_role_role_guid);
GO
