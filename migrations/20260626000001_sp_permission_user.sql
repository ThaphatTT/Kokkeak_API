-- =============================================================================
-- M17: Permission-page stored procedures.
-- -----------------------------------------------------------------------------
-- Decouples the permission flow from `dbo.SP_PERMISSION_USER_FIND_BY_USERNAME`
-- (the M16 SP that also backed the admin user-management screen). The new
-- SPs:
--
--   - take GUIDs directly (no GUID→username translation in Rust)
--   - return a single `user_role_name` string instead of CSV
--   - return `has_override` / `effective_status` as `INT` (0/1)
--
-- Both SPs are read-only and live in KOKKAK_MASTER (same DB as the M16 SPs).
--
-- M19 follow-up: the original M17 SP was named `SP_PERMISSION_USER_LIST_V2`
-- (intentionally suffixed to coexist with the legacy M16
-- `SP_PERMISSION_USER_LIST`). M19 renames the M17 SP to the canonical
-- `SP_PERMISSION_USER_LIST` name and merges the M16 column set into the
-- returned row shape so a single SP backs both the admin console and the
-- permission page. The implementation + `@p_user_guid` admin gate live in
-- `20260628000001_sp_caller_user_guid.sql` — this file keeps the stub
-- `CREATE PROCEDURE` so a fresh install from scratch doesn't 500 on
-- `ALTER PROCEDURE` of a non-existent object before the M19 migration runs.
-- =============================================================================

-- ----------------------------------------------------------------------------
-- SP_PERMISSION_USER_LIST  (canonical — see M19 for body + @p_user_guid)
-- ----------------------------------------------------------------------------
IF OBJECT_ID('dbo.SP_PERMISSION_USER_LIST', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.SP_PERMISSION_USER_LIST AS BEGIN SET NOCOUNT ON; END');
GO

-- ----------------------------------------------------------------------------
-- SP_PERMISSION_USER_DETAIL_FIND_BY_GUID
-- ----------------------------------------------------------------------------
-- Per-user detailed permission rows (one per `(user, permission)` pair).
-- Takes a GUID directly (no GUID→username translation needed in Rust).
--
-- Returns:
--   - One row per `(user, catalog-permission)` pair (catalog × user expansion)
--   - `effective_status = 0` when an explicit deny wins, `1` otherwise
--   - `has_override = 1` when the user has an explicit allow/deny override
--
-- Result columns:
--   user_guid             UNIQUEIDENTIFIER
--   full_name             NVARCHAR(201)
--   email                 NVARCHAR(255)
--   user_role_name        NVARCHAR(64)
--   user_permission_code  NVARCHAR(64)
--   user_permission_name  NVARCHAR(128)
--   has_override          INT               (0 / 1)
--   override_effect       NVARCHAR(16)      ('allow' | 'deny' | '')
--   effective_status      INT               (0 / 1)
--
-- Errors:
--   No rows  → user_guid doesn't resolve to a user (caller maps to 404).
-- ----------------------------------------------------------------------------
IF OBJECT_ID('dbo.SP_PERMISSION_USER_DETAIL_FIND_BY_GUID', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.SP_PERMISSION_USER_DETAIL_FIND_BY_GUID AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.SP_PERMISSION_USER_DETAIL_FIND_BY_GUID
    @p_user_guid UNIQUEIDENTIFIER
AS
BEGIN
    SET NOCOUNT ON;

    -- Profile lookup (raises nothing if missing — caller treats empty
    -- result set as 404 per the trait contract).
    DECLARE @user_full_name NVARCHAR(201) =
        LTRIM(RTRIM(ISNULL(
            (SELECT u.user_first_name + ' ' + u.user_last_name
             FROM [user] u WHERE u.user_guid = @p_user_guid),
            '')));
    DECLARE @user_email NVARCHAR(255) =
        ISNULL(
            (SELECT un.user_username_username
             FROM [user_username] un
             WHERE un.user_username_user_guid = @p_user_guid
               AND un.user_username_status = 1),
            '');
    DECLARE @user_role_name NVARCHAR(64) =
        ISNULL(
            (SELECT TOP 1 r.user_role_name
             FROM [user_user_role] ur
             JOIN [user_role] r
                 ON r.user_role_guid = ur.user_user_role_role_guid
                AND r.user_role_status = 1
             WHERE ur.user_user_role_user_guid = @p_user_guid
               AND ur.user_user_role_status = 1
             ORDER BY r.user_role_code),
            '');

    -- Catalog expansion × role grants × explicit overrides.
    SELECT
        @p_user_guid AS user_guid,
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
       AND ur.user_user_role_user_guid = @p_user_guid
       AND ur.user_user_role_status = 1
    LEFT JOIN [user_permission_override] ov
        ON ov.user_permission_override_user_guid = @p_user_guid
       AND ov.user_permission_override_permission_guid = p.user_permission_guid
    WHERE p.user_permission_status = 1
    ORDER BY p.user_permission_code;
END;
GO
