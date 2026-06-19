-- ============================================================================
-- KOKKAK NEW_DB v2 - Stored Procedures: User & Auth
--
-- Reference: kokkeak/NEW_DB.txt (4 tables) + KOKKEAK_MIGRATION_PLAN/02_DATABASE.md
--
-- Output convention (uniform across all API_* SPs):
--   Every SP returns ONE result set whose column shape depends on the op.
--   For write operations, the result row is:
--       <primary_value_column>, error_code INT, error_message NVARCHAR(255)
--   error_code: 0 = ok, 1 = not_found, 2 = conflict, 3 = bad_input
--   For read operations, regular columns followed by error_code + error_message.
--   The Rust side reads the first row and maps error_code to RepoError.
--
-- Why this shape (not OUTPUT params)?
--   tiberius's RPC support for OUTPUT params is spotty across patch
--   versions. A single SELECT result set is portable + trivially testable.
-- ============================================================================

-- ----------------------------------------------------------------------------
-- API_USER_REGISTER
-- Returns: (user_guid, error_code, error_message). 0 = ok.
-- ----------------------------------------------------------------------------
IF OBJECT_ID('dbo.API_USER_REGISTER', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.API_USER_REGISTER AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.API_USER_REGISTER
    @p_first_name      NVARCHAR(100),
    @p_last_name       NVARCHAR(100),
    @p_username        NVARCHAR(255),
    @p_password_hash   NVARCHAR(512),
    @p_role_code       NVARCHAR(32)
AS
BEGIN
    SET NOCOUNT ON;
    DECLARE @new_guid UNIQUEIDENTIFIER = NEWID();
    DECLARE @role_guid UNIQUEIDENTIFIER;

    BEGIN TRY
        BEGIN TRAN;

        -- Lookup role
        SELECT @role_guid = user_role_guid
        FROM [user_role]
        WHERE user_role_code = @p_role_code AND user_role_status = 1;
        IF @role_guid IS NULL
        BEGIN
            ROLLBACK TRAN;
            SELECT CAST(NULL AS UNIQUEIDENTIFIER) AS user_guid,
                   3 AS error_code,
                   N'role not found: ' + @p_role_code AS error_message;
            RETURN;
        END;

        -- Profile row
        INSERT INTO [user] (
            user_guid, user_first_name, user_last_name,
            user_status, user_create_at, user_create_by,
            user_update_at, user_update_by
        ) VALUES (
            @new_guid, @p_first_name, @p_last_name,
            1, SYSUTCDATETIME(), @new_guid,
            SYSUTCDATETIME(), @new_guid
        );

        -- Credentials row
        INSERT INTO [user_username] (
            user_username_guid, user_username_user_guid,
            user_username_username, user_username_password,
            user_username_status,
            user_username_create_at, user_username_create_by,
            user_username_update_at, user_username_update_by
        ) VALUES (
            NEWID(), @new_guid, LOWER(@p_username), @p_password_hash,
            1, SYSUTCDATETIME(), @new_guid,
            SYSUTCDATETIME(), @new_guid
        );

        -- Role assignment
        INSERT INTO [user_user_role] (
            user_user_role_guid, user_user_role_user_guid, user_user_role_role_guid,
            user_user_role_status, user_user_role_assigned_by, user_user_role_assigned_at,
            user_user_role_create_at, user_user_role_create_by,
            user_user_role_update_at, user_user_role_update_by
        ) VALUES (
            NEWID(), @new_guid, @role_guid,
            1, @new_guid, SYSUTCDATETIME(),
            SYSUTCDATETIME(), @new_guid,
            SYSUTCDATETIME(), @new_guid
        );

        COMMIT TRAN;
        SELECT @new_guid AS user_guid, 0 AS error_code, '' AS error_message;
    END TRY
    BEGIN CATCH
        IF @@TRANCOUNT > 0 ROLLBACK TRAN;
        IF ERROR_NUMBER() IN (2627, 2601)
            SELECT CAST(NULL AS UNIQUEIDENTIFIER) AS user_guid,
                   2 AS error_code,
                   N'username already taken' AS error_message;
        ELSE
            SELECT CAST(NULL AS UNIQUEIDENTIFIER) AS user_guid,
                   3 AS error_code,
                   ERROR_MESSAGE() AS error_message;
    END CATCH;
END;
GO

-- ----------------------------------------------------------------------------
-- API_USER_FIND_BY_USERNAME
-- Result: profile row + second result set = role codes (comma-separated).
--          Empty result set when not found.
-- ----------------------------------------------------------------------------
IF OBJECT_ID('dbo.API_USER_FIND_BY_USERNAME', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.API_USER_FIND_BY_USERNAME AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.API_USER_FIND_BY_USERNAME
    @p_username NVARCHAR(255)
AS
BEGIN
    SET NOCOUNT ON;

    SELECT
        u.user_guid                 AS id,
        u.user_first_name           AS first_name,
        u.user_last_name            AS last_name,
        un.user_username_username   AS username,
        un.user_username_password   AS password_hash,
        u.user_status               AS status,
        u.user_create_at            AS created_at,
        u.user_update_at            AS updated_at
    FROM [user] u
    INNER JOIN [user_username] un
        ON un.user_username_user_guid = u.user_guid
    WHERE u.user_status <> 3
      AND LOWER(un.user_username_username) = LOWER(@p_username);

    -- Roles (separate result set; empty if user not found)
    SELECT STUFF((
        SELECT ',' + ur.user_role_code
        FROM [user_user_role] uur
        INNER JOIN [user_role] ur
            ON ur.user_role_guid = uur.user_user_role_role_guid
        INNER JOIN [user_username] un
            ON un.user_username_user_guid = uur.user_user_role_user_guid
        WHERE LOWER(un.user_username_username) = LOWER(@p_username)
          AND uur.user_user_role_status = 1
        FOR XML PATH('')
    ), 1, 1, '') AS role_codes;
END;
GO

-- ----------------------------------------------------------------------------
-- API_USER_FIND_BY_ID (same shape as above)
-- ----------------------------------------------------------------------------
IF OBJECT_ID('dbo.API_USER_FIND_BY_ID', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.API_USER_FIND_BY_ID AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.API_USER_FIND_BY_ID
    @p_user_guid UNIQUEIDENTIFIER
AS
BEGIN
    SET NOCOUNT ON;

    SELECT
        u.user_guid                 AS id,
        u.user_first_name           AS first_name,
        u.user_last_name            AS last_name,
        un.user_username_username   AS username,
        un.user_username_password   AS password_hash,
        u.user_status               AS status,
        u.user_create_at            AS created_at,
        u.user_update_at            AS updated_at
    FROM [user] u
    INNER JOIN [user_username] un
        ON un.user_username_user_guid = u.user_guid
    WHERE u.user_status <> 3
      AND u.user_guid = @p_user_guid;

    SELECT STUFF((
        SELECT ',' + ur.user_role_code
        FROM [user_user_role] uur
        INNER JOIN [user_role] ur
            ON ur.user_role_guid = uur.user_user_role_role_guid
        WHERE uur.user_user_role_user_guid = @p_user_guid
          AND uur.user_user_role_status = 1
        FOR XML PATH('')
    ), 1, 1, '') AS role_codes;
END;
GO

-- ----------------------------------------------------------------------------
-- API_USER_UPDATE
-- Result: (user_guid, error_code, error_message). error_code 1 = not found.
-- ----------------------------------------------------------------------------
IF OBJECT_ID('dbo.API_USER_UPDATE', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.API_USER_UPDATE AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.API_USER_UPDATE
    @p_user_guid       UNIQUEIDENTIFIER,
    @p_first_name      NVARCHAR(100),
    @p_last_name       NVARCHAR(100),
    @p_password_hash   NVARCHAR(512),
    @p_status          INT
AS
BEGIN
    SET NOCOUNT ON;

    UPDATE [user]
    SET user_first_name = @p_first_name,
        user_last_name  = @p_last_name,
        user_status     = @p_status,
        user_update_at  = SYSUTCDATETIME(),
        user_update_by  = @p_user_guid
    WHERE user_guid = @p_user_guid;

    IF @@ROWCOUNT = 0
    BEGIN
        SELECT @p_user_guid AS user_guid, 1 AS error_code, N'user not found' AS error_message;
        RETURN;
    END;

    UPDATE [user_username]
    SET user_username_password   = @p_password_hash,
        user_username_update_at  = SYSUTCDATETIME(),
        user_username_update_by  = @p_user_guid
    WHERE user_username_user_guid = @p_user_guid;

    IF @@ROWCOUNT = 0
    BEGIN
        SELECT @p_user_guid AS user_guid, 1 AS error_code, N'credentials not found' AS error_message;
        RETURN;
    END;

    SELECT @p_user_guid AS user_guid, 0 AS error_code, '' AS error_message;
END;
GO

-- ----------------------------------------------------------------------------
-- API_USER_SET_ROLES (replace role set — used by admin endpoints M15+)
-- Result: (user_guid, error_code, error_message). 0 = ok, 1 = no-op.
-- ----------------------------------------------------------------------------
IF OBJECT_ID('dbo.API_USER_SET_ROLES', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.API_USER_SET_ROLES AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.API_USER_SET_ROLES
    @p_user_guid   UNIQUEIDENTIFIER,
    @p_role_codes  NVARCHAR(MAX)
AS
BEGIN
    SET NOCOUNT ON;
    BEGIN TRAN;

    UPDATE [user_user_role]
    SET user_user_role_status = 0,
        user_user_role_update_at = SYSUTCDATETIME()
    WHERE user_user_role_user_guid = @p_user_guid;

    DECLARE @code NVARCHAR(32);
    DECLARE @role_guid UNIQUEIDENTIFIER;
    DECLARE @pos INT = 1, @next INT, @len INT = LEN(@p_role_codes);

    WHILE @pos <= @len
    BEGIN
        SET @next = CHARINDEX(',', @p_role_codes, @pos);
        IF @next = 0 SET @next = @len + 1;
        SET @code = LTRIM(RTRIM(SUBSTRING(@p_role_codes, @pos, @next - @pos)));
        IF LEN(@code) > 0
        BEGIN
            SELECT @role_guid = user_role_guid
            FROM [user_role]
            WHERE user_role_code = @code AND user_role_status = 1;
            IF @role_guid IS NOT NULL
            BEGIN
                INSERT INTO [user_user_role] (
                    user_user_role_guid, user_user_role_user_guid, user_user_role_role_guid,
                    user_user_role_status, user_user_role_assigned_by, user_user_role_assigned_at,
                    user_user_role_create_at, user_user_role_create_by,
                    user_user_role_update_at, user_user_role_update_by
                ) VALUES (
                    NEWID(), @p_user_guid, @role_guid,
                    1, @p_user_guid, SYSUTCDATETIME(),
                    SYSUTCDATETIME(), @p_user_guid,
                    SYSUTCDATETIME(), @p_user_guid
                );
            END;
        END;
        SET @pos = @next + 1;
    END;

    COMMIT TRAN;
    SELECT @p_user_guid AS user_guid, 0 AS error_code, '' AS error_message;
END;
GO
