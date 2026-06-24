-- ============================================================================
-- KOKKAK NEW_DB v2 - Stored Procedures: User Role / Permission (M15-prep)
--
-- Reference: RDBMS Permssion.md §3 (Permission resolution algorithm) +
-- NEW_DB.txt `user_role_permission` + `user_permission` tables.
--
-- Creates:
--   1. [user_permission]         - permission catalog (SCREAMING_SNAKE codes)
--   2. [user_role_permission]    - M:N role ↔ permission junction
--   3. dbo.SP_USER_GROUP_ROLE    - read-side SP for the admin "role+permission"
--                                 matrix endpoint, supporting two modes:
--                                   * mode = 'SELECT'     → all active roles
--                                       (LEFT JOIN so roles with no permissions
--                                        still appear)
--                                   * mode = 'SELECT_ID'  → only the role whose
--                                       user_role_guid = @p_user_role_guid
--
-- Mode values are intentionally short to match the API convention
-- (handlers pass `?mode=SELECT|SELECT_ID` via a typed enum).
--
-- ponytail: the SP returns ONE result set with one row per
-- (role × permission) pair. COALESCE on the LEFT-JOINed columns
-- gives empty strings / zero status when a role has no
-- permission yet, so the Rust side can deserialize into the same
-- DTO for both modes without conditional mapping. Ceiling: when
-- the permission catalog needs versioning (introducing v2 codes
-- alongside v1) we add a `user_permission_version` column +
-- SP parameter — out of scope for M15.
-- ============================================================================

-- ----------------------------------------------------------------------------
-- 1. [user_permission] - permission catalog
-- ----------------------------------------------------------------------------
IF OBJECT_ID('[user_permission]', 'U') IS NULL
CREATE TABLE [user_permission] (
    user_permission_guid       UNIQUEIDENTIFIER NOT NULL PRIMARY KEY,
    user_permission_code       NVARCHAR(64)     NOT NULL,
    user_permission_name       NVARCHAR(100)    NOT NULL,
    user_permission_module     NVARCHAR(32)     NULL,    -- e.g. 'PAGE', 'JOBS'
    user_permission_description NVARCHAR(MAX)    NULL,
    user_permission_status     INT              NOT NULL DEFAULT 1,
    user_permission_create_at  DATETIME2(7)     NOT NULL DEFAULT SYSUTCDATETIME(),
    user_permission_create_by  UNIQUEIDENTIFIER NULL REFERENCES [user](user_guid),
    user_permission_update_at  DATETIME2(7)     NOT NULL DEFAULT SYSUTCDATETIME(),
    user_permission_update_by  UNIQUEIDENTIFIER NULL REFERENCES [user](user_guid),
    CONSTRAINT uq_user_permission_code UNIQUE (user_permission_code)
);
GO

IF NOT EXISTS (SELECT 1 FROM sys.indexes WHERE name = 'ix_user_permission_status')
CREATE INDEX ix_user_permission_status ON [user_permission](user_permission_status);
GO

-- ----------------------------------------------------------------------------
-- 2. [user_role_permission] - M:N role ↔ permission junction
-- ----------------------------------------------------------------------------
IF OBJECT_ID('[user_role_permission]', 'U') IS NULL
CREATE TABLE [user_role_permission] (
    user_role_permission_guid        UNIQUEIDENTIFIER NOT NULL PRIMARY KEY,
    user_role_permission_role_guid   UNIQUEIDENTIFIER NOT NULL REFERENCES [user_role](user_role_guid),
    user_role_permission_permission_guid UNIQUEIDENTIFIER NOT NULL REFERENCES [user_permission](user_permission_guid),
    user_role_permission_status      INT              NOT NULL DEFAULT 1,
    user_role_permission_granted_by  UNIQUEIDENTIFIER NULL REFERENCES [user](user_guid),
    user_role_permission_granted_at  DATETIME2(7)     NULL,
    user_role_permission_create_at   DATETIME2(7)     NOT NULL DEFAULT SYSUTCDATETIME(),
    user_role_permission_create_by   UNIQUEIDENTIFIER NULL REFERENCES [user](user_guid),
    user_role_permission_update_at   DATETIME2(7)     NOT NULL DEFAULT SYSUTCDATETIME(),
    user_role_permission_update_by   UNIQUEIDENTIFIER NULL REFERENCES [user](user_guid),
    CONSTRAINT uq_user_role_permission_pair
        UNIQUE (user_role_permission_role_guid, user_role_permission_permission_guid)
);
GO

IF NOT EXISTS (SELECT 1 FROM sys.indexes WHERE name = 'ix_user_role_permission_role_guid')
CREATE INDEX ix_user_role_permission_role_guid ON [user_role_permission](user_role_permission_role_guid);
GO

IF NOT EXISTS (SELECT 1 FROM sys.indexes WHERE name = 'ix_user_role_permission_permission_guid')
CREATE INDEX ix_user_role_permission_permission_guid ON [user_role_permission](user_role_permission_permission_guid);
GO

-- ----------------------------------------------------------------------------
-- 3. dbo.SP_USER_GROUP_ROLE - role × permission matrix (admin view)
-- ----------------------------------------------------------------------------
-- The SP takes ONE parameter: @p_mode (a literal pass-through from
-- the API). The mode is a free-form text discriminator the
-- admin UI uses to scope which roles appear in the matrix
-- (e.g. `SELECT_ADMIN` for the admin-role view, `SELECT_EMPLOYEE`
-- for the employee-role view). The mode values are
-- application-defined and may be extended; the SP just
-- checks membership against the supported set.
--
-- Currently the SP supports exactly 2 mode values:
--   - 'SELECT_ADMIN'    → return roles for admin / super_admin
--   - 'SELECT_EMPLOYEE' → return roles for customer / technician
--                          (i.e. everyone who is NOT an admin)
--
-- Unknown mode values return zero rows (graceful failure —
-- the handler doesn't need to know the exact whitelist; it
-- just passes the mode through as a string).
--
-- ponytail: the SP is the source of truth for the matrix
-- (filters, JOIN order, status gates). The Rust caller only
-- reshapes — no business logic.
-- ----------------------------------------------------------------------------
IF OBJECT_ID('dbo.SP_USER_GROUP_ROLE', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.SP_USER_GROUP_ROLE AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.SP_USER_GROUP_ROLE
    @p_mode NVARCHAR(32)   -- pass-through mode literal from the API
AS
BEGIN
    SET NOCOUNT ON;

    -- Two supported mode values (the 2 conditions the user
    -- expects). Each branch filters the role set differently:
    --   SELECT_ADMIN    → admin + super_admin roles
    --   SELECT_EMPLOYEE → everyone else (customer, technician, ...)
    -- Unknown mode values return 0 rows (graceful).
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
          -- Condition 1: admin-type roles
          (@p_mode = 'SELECT_ADMIN'
              AND ur.user_role_code IN ('admin', 'super_admin'))
          -- Condition 2: non-admin (employee-type) roles
          OR (@p_mode = 'SELECT_EMPLOYEE'
              AND ur.user_role_code NOT IN ('admin', 'super_admin'))
      )
      AND (
          urp.user_role_permission_guid IS NULL              -- role with no permissions yet
          OR urp.user_role_permission_status = 1             -- assignment still active
      )
    ORDER BY ur.user_role_code, up.user_permission_code;
END;
GO
