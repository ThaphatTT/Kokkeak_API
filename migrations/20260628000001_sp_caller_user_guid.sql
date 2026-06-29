-- ============================================================================
-- M19: SP_-prefixed read stored procedures receive `@p_user_guid` of the
-- caller for permission check + audit.
-- -----------------------------------------------------------------------------
-- Three SPs that back admin / permission-page endpoints are extended with
-- a uniform `@p_user_guid UNIQUEIDENTIFIER = NULL` parameter:
--
--   1. SP_PERMISSION_USER_LIST            (was V2 — renamed in M19)
--   2. SP_PERMISSION_USER_DETAIL_FIND_BY_GUID
--   3. SP_USER_GROUP_ROLE
--
-- Why a generic `@p_user_guid` (not `@p_update_by`):
--
--   `@p_update_by` only fits writes. Reads / creates / updates / deletes
--   are all authorised actors — the naming must be the same across CRUD
--   so the SP signature stays uniform. `user_guid` reads as "the user
--   performing the operation" regardless of verb.
--
-- Permission rule (uniform across the three SPs):
--
--   - `@p_user_guid` IS NULL         → "system actor", allow access.
--                                       Kept as a forward-compat escape
--                                       hatch for batch jobs / migrations
--                                       that legitimately have no caller.
--   - `@p_user_guid` is a real GUID  → the SP verifies that the GUID
--                                       belongs to a user holding an
--                                       active admin or super_admin
--                                       role. If not, the SELECT
--                                       returns zero rows (fail-closed).
--                                       Empty result matches the trait
--                                       contract for read SPs.
--
-- The Rust caller ALWAYS passes a non-null GUID (`user.id()` from the
-- JWT extractor). The NULL branch is for the DB migration tool only.
--
-- The check uses an EXISTS subquery against `user_user_role` + `user_role`
-- with `user_role_code IN ('admin','super_admin')`. The legacy admin
-- definition in `20260620000007_sp_user_role.sql` (`SP_USER_GROUP_ROLE`)
-- uses the exact same predicate — both SPs agree on what "admin" means.
--
-- Scope (M19, locked):
--
--   - Touches `SP_*` SPs only. The legacy `API_*` SPs (e.g.
--     `API_USER_FIND_BY_USERNAME`) keep their shape; they back the
--     login flow where the caller is unauthenticated and `@p_user_guid`
--     isn't applicable.
--   - Out of scope: `API_CHAT_*`, `API_ORDER_*`, `API_PAYMENT_*`, and
--     other per-user SPs where the caller IS the user — the JWT
--     extractor already gates access at the axum layer.
--
-- Shape note (legacy carryover):
--
--   `MssqlUserRepository::list_with_permissions` (used by
--   `GET /api/v1/admin/users`) currently calls `SP_PERMISSION_USER_LIST`
--   — the legacy M16 SP whose definition was dropped during the M17
--   cleanup. The V2 SP (`SP_PERMISSION_USER_LIST_V2`) took its place at
--   the SQL level but with the trimmed column set (no `role_codes`,
--   no `has_permission`, ...). To make this PR self-contained without
--   rewriting the admin user-list DTO, the renamed SP emits BOTH
--   shapes — V1 (admin console) and V2 (permission page) columns are
--   present in one row per user. The duplication is cheap and avoids
--   a domain DTO refactor on a security PR.
-- ============================================================================

-- ----------------------------------------------------------------------------
-- 1) Drop the M17 V2 alias. We re-create as the canonical `SP_PERMISSION_USER_LIST`.
-- ----------------------------------------------------------------------------
IF OBJECT_ID('dbo.SP_PERMISSION_USER_LIST_V2', 'P') IS NOT NULL
BEGIN
    DROP PROCEDURE dbo.SP_PERMISSION_USER_LIST_V2;
END;
GO

-- ----------------------------------------------------------------------------
-- 2) SP_PERMISSION_USER_LIST (canonical — was V2 + legacy V1 columns merged)
-- ----------------------------------------------------------------------------
-- One row per user. Returns the union of the M16 round 2 admin-console
-- columns (`role_codes` / `role_names` / `has_permission` / `has_override`
-- / `user_username_status`) AND the M17 permission-page columns
-- (`user_role_name` singular, `user_create_at`, `user_update_at`). The
-- admin console mapper ignores the M17 columns; the permission-page
-- mapper ignores the M16 columns. The wire payload stays stable for
-- both DTOs until a future cleanup unifies them.
--
-- Parameters:
--   @p_user_guid UNIQUEIDENTIFIER  (caller — see file header for rule)
--
-- Result columns (per row):
--   user_guid              UNIQUEIDENTIFIER
--   full_name              NVARCHAR(201)
--   email                  NVARCHAR(255)
--   user_role_name         NVARCHAR(64)            -- M17 (single canonical role)
--   role_codes             NVARCHAR(MAX)           -- M16 (CSV)
--   role_names             NVARCHAR(MAX)           -- M16 (CSV)
--   has_permission         INT (0/1)               -- M16
--   has_override           INT (0/1)               -- M16
--   user_status            INT                     -- [user].user_status
--   user_username_status   INT                     -- [user_username].user_username_status
--   user_create_at         datetime2(7)
--   user_update_at         datetime2(7)
IF OBJECT_ID('dbo.SP_PERMISSION_USER_LIST', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.SP_PERMISSION_USER_LIST AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.SP_PERMISSION_USER_LIST
    -- Project convention: GUIDs into `dbo.SP_*` arrive as `varchar(36)`
    -- (the hyphenated UUID string Rust emits via `Uuid::to_string()`).
    -- The CAST happens once below — body uses `@p_user_guid_uid` as the
    -- native `uniqueidentifier`. See `crates/infra/src/db/mssql_*.rs`
    -- for the caller-side `&str` binding.
    @p_user_guid varchar(36) = NULL
AS
BEGIN
    SET NOCOUNT ON;

    DECLARE @p_user_guid_uid uniqueidentifier =
        CASE WHEN @p_user_guid IS NULL OR LTRIM(RTRIM(@p_user_guid)) = ''
             THEN NULL
             ELSE CAST(@p_user_guid AS uniqueidentifier) END;

    -- Admin gate (skipped when @p_user_guid_uid IS NULL — system actor).
    IF @p_user_guid_uid IS NOT NULL
       AND NOT EXISTS (
            SELECT 1
            FROM [user_user_role] ur
            JOIN [user_role] r
                ON r.user_role_guid = ur.user_user_role_role_guid
               AND r.user_role_status = 1
               AND r.user_role_code IN ('admin','super_admin')
            WHERE ur.user_user_role_user_guid = @p_user_guid_uid
              AND ur.user_user_role_status   = 1
       )
    BEGIN
        -- Fail-closed: non-admin caller sees zero rows. The trait
        -- contract for "not found / no rows" returns `Ok(empty)` or
        -- `NotFound` depending on adapter — both are safe by design.
        RETURN;
    END;

    ;WITH user_active_roles AS (
        SELECT
            u.user_guid,
            -- M16 CSV aggregates
            STRING_AGG(r.user_role_code, ',')
                WITHIN GROUP (ORDER BY r.user_role_code) AS role_codes_csv,
            STRING_AGG(r.user_role_name, ',')
                WITHIN GROUP (ORDER BY r.user_role_code) AS role_names_csv,
            -- M17 canonical single role name
            MAX(CASE WHEN r.user_role_code = (
                        SELECT TOP 1 r2.user_role_code
                        FROM [user_user_role] ur2
                        JOIN [user_role] r2
                            ON r2.user_role_guid = ur2.user_user_role_role_guid
                           AND r2.user_role_status = 1
                        WHERE ur2.user_user_role_user_guid = u.user_guid
                          AND ur2.user_user_role_status   = 1
                        ORDER BY r2.user_role_code
                    )
                    THEN r.user_role_name END) AS canonical_role_name,
            -- Permission summary (used by both DTOs: list-with-permissions
            -- mapper and the `has_permission` badge)
            MAX(CASE WHEN rg.user_role_permission_guid IS NOT NULL THEN 1 ELSE 0 END)
                AS has_permission_bit
        FROM [user] u
        JOIN [user_user_role] ur
            ON ur.user_user_role_user_guid = u.user_guid
           AND ur.user_user_role_status = 1
        JOIN [user_role] r
            ON r.user_role_guid = ur.user_user_role_role_guid
           AND r.user_role_status = 1
        LEFT JOIN [user_role_permission] rg
            ON rg.user_role_permission_role_guid = r.user_role_guid
           AND rg.user_role_permission_status  = 1
        WHERE u.user_status = 1
        GROUP BY u.user_guid
    ),
    user_override AS (
        SELECT
            user_permission_override_user_guid AS user_guid,
            1 AS has_override_bit
        FROM [user_permission_override]
        WHERE user_permission_override_status = 1
        GROUP BY user_permission_override_user_guid
    )
    SELECT
        u.user_guid,
        LTRIM(RTRIM(ISNULL(u.user_first_name,'') + ' ' + ISNULL(u.user_last_name,''))) AS full_name,
        un.user_username_username AS email,
        ur.canonical_role_name                  AS user_role_name,
        ur.role_codes_csv                       AS role_codes,
        ur.role_names_csv                       AS role_names,
        ISNULL(ur.has_permission_bit, 0)        AS has_permission,
        ISNULL(ov.has_override_bit, 0)          AS has_override,
        u.user_status,
        un.user_username_status,
        u.user_create_at,
        COALESCE(u.user_update_at, u.user_create_at) AS user_update_at
    FROM [user] u
    JOIN [user_username] un
        ON un.user_username_user_guid = u.user_guid
       AND un.user_username_status = 1
    LEFT JOIN user_active_roles ur
        ON ur.user_guid = u.user_guid
    LEFT JOIN user_override ov
        ON ov.user_guid = u.user_guid
    WHERE u.user_status = 1
    ORDER BY un.user_username_username;
END;
GO

-- ----------------------------------------------------------------------------
-- 3) SP_PERMISSION_USER_DETAIL_FIND_BY_GUID — add @p_user_guid caller check
-- ----------------------------------------------------------------------------
-- Same admin gate rule as above. Project convention: GUIDs arrive as
-- `varchar(36)` (CAST inside). The two callers on this SP are:
--   @p_user_guid          varchar(36)   -- lookup target (existing)
--   @p_caller_user_guid   varchar(36)   -- admin gate (M19)
-- The body casts both to `uniqueidentifier` once, into local variables,
-- and uses those locals for every JOIN.
IF OBJECT_ID('dbo.SP_PERMISSION_USER_DETAIL_FIND_BY_GUID', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.SP_PERMISSION_USER_DETAIL_FIND_BY_GUID AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.SP_PERMISSION_USER_DETAIL_FIND_BY_GUID
    @p_user_guid         varchar(36),            -- lookup target
    @p_caller_user_guid  varchar(36) = NULL       -- admin gate (M19)
AS
BEGIN
    SET NOCOUNT ON;

    -- CAST string GUIDs to uniqueidentifier once (NULL/blank → NULL).
    DECLARE @user_guid_uid uniqueidentifier =
        TRY_CAST(NULLIF(LTRIM(RTRIM(@p_user_guid)), '') AS uniqueidentifier);
    DECLARE @caller_guid_uid uniqueidentifier =
        TRY_CAST(NULLIF(LTRIM(RTRIM(@p_caller_user_guid)), '') AS uniqueidentifier);

    -- Admin gate (see SP_PERMISSION_USER_LIST for the rule).
    IF @caller_guid_uid IS NOT NULL
       AND NOT EXISTS (
            SELECT 1
            FROM [user_user_role] ur
            JOIN [user_role] r
                ON r.user_role_guid = ur.user_user_role_role_guid
               AND r.user_role_status = 1
               AND r.user_role_code IN ('admin','super_admin')
            WHERE ur.user_user_role_user_guid = @caller_guid_uid
              AND ur.user_user_role_status   = 1
       )
    BEGIN
        RETURN;
    END;

    -- Profile lookup (raises nothing if missing — caller treats empty
    -- result set as 404 per the trait contract).
    DECLARE @user_full_name NVARCHAR(201) =
        LTRIM(RTRIM(ISNULL(
            (SELECT u.user_first_name + ' ' + u.user_last_name
             FROM [user] u WHERE u.user_guid = @user_guid_uid),
            '')));
    DECLARE @user_email NVARCHAR(255) =
        ISNULL(
            (SELECT un.user_username_username
             FROM [user_username] un
             WHERE un.user_username_user_guid = @user_guid_uid
               AND un.user_username_status = 1),
            '');
    DECLARE @user_role_name NVARCHAR(64) =
        ISNULL(
            (SELECT TOP 1 r.user_role_name
             FROM [user_user_role] ur
             JOIN [user_role] r
                 ON r.user_role_guid = ur.user_user_role_role_guid
                AND r.user_role_status = 1
             WHERE ur.user_user_role_user_guid = @user_guid_uid
               AND ur.user_user_role_status = 1
             ORDER BY r.user_role_code),
            '');

    -- Catalog expansion × role grants × explicit overrides.
    SELECT
        @user_guid_uid AS user_guid,
        @user_full_name AS full_name,
        @user_email AS email,
        @user_role_name AS user_role_name,
        p.user_permission_code,
        p.user_permission_name,
        CAST(
            CASE WHEN ov.user_permission_override_guid IS NULL THEN 0 ELSE 1 END
        AS INT) AS has_override,
        ISNULL(ov.user_permission_override_effect, '') AS override_effect,
        CAST(
            CASE
                WHEN ov.user_permission_override_effect = 'deny' THEN 0
                WHEN rg.user_role_permission_guid IS NOT NULL THEN 1
                WHEN ov.user_permission_override_effect = 'allow' THEN 1
                ELSE 0
            END
        AS INT) AS effective_status
    FROM [user_permission] p
    LEFT JOIN [user_role_permission] rg
        ON rg.user_role_permission_permission_guid = p.user_permission_guid
    LEFT JOIN [user_user_role] ur
        ON ur.user_user_role_role_guid = rg.user_role_permission_role_guid
       AND ur.user_user_role_user_guid = @user_guid_uid
       AND ur.user_user_role_status = 1
    LEFT JOIN [user_permission_override] ov
        ON ov.user_permission_override_user_guid = @user_guid_uid
       AND ov.user_permission_override_permission_guid = p.user_permission_guid
    WHERE p.user_permission_status = 1
    ORDER BY p.user_permission_code;
END;
GO

-- ----------------------------------------------------------------------------
-- 4) SP_USER_GROUP_ROLE — add @p_user_guid caller check
-- ----------------------------------------------------------------------------
-- Same admin gate. The SP's existing body is unchanged; only the new
-- parameter + the early-RETURN on non-admin is added at the top.
IF OBJECT_ID('dbo.SP_USER_GROUP_ROLE', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.SP_USER_GROUP_ROLE AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.SP_USER_GROUP_ROLE
    @p_mode      NVARCHAR(32),       -- pass-through mode literal from the API
    @p_user_guid varchar(36) = NULL  -- admin gate (M19; string per project convention)
AS
BEGIN
    SET NOCOUNT ON;

    DECLARE @p_user_guid_uid uniqueidentifier =
        TRY_CAST(NULLIF(LTRIM(RTRIM(@p_user_guid)), '') AS uniqueidentifier);

    -- Admin gate (same rule as the other two SPs).
    IF @p_user_guid_uid IS NOT NULL
       AND NOT EXISTS (
            SELECT 1
            FROM [user_user_role] ur
            JOIN [user_role] r
                ON r.user_role_guid = ur.user_user_role_role_guid
               AND r.user_role_status = 1
               AND r.user_role_code IN ('admin','super_admin')
            WHERE ur.user_user_role_user_guid = @p_user_guid_uid
              AND ur.user_user_role_status   = 1
       )
    BEGIN
        RETURN;
    END;

    SELECT
        COALESCE(ur.user_role_guid, '')                          AS user_role_guid,
        COALESCE(ur.user_role_code, '')                          AS user_role_code,
        COALESCE(urp.user_role_permission_guid, '')              AS user_role_permission_guid,
        COALESCE(urp.user_role_permission_status, 0)             AS user_role_permission_status,
        COALESCE(up.user_permission_guid, '')                    AS user_permission_guid,
        COALESCE(up.user_permission_code, '')                    AS user_permission_code
    FROM [user_role] ur
    LEFT JOIN [user_role_permission] urp
        ON urp.user_role_permission_role_guid = ur.user_role_guid
    LEFT JOIN [user_permission] up
        ON up.user_permission_guid = urp.user_role_permission_permission_guid
        AND up.user_permission_status = 1
    WHERE ur.user_role_status = 1
      AND (
          (@p_mode = 'SELECT_ADMIN'
              AND ur.user_role_code IN ('admin', 'super_admin'))
          OR (@p_mode = 'SELECT_EMPLOYEE'
              AND ur.user_role_code NOT IN ('admin', 'super_admin'))
      )
      AND (
          urp.user_role_permission_guid IS NULL
          OR urp.user_role_permission_status = 1
      )
    ORDER BY ur.user_role_code, up.user_permission_code;
END;
GO
