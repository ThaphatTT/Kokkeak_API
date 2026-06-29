-- 20260630000001_sp_user_find_by_id.sql
--
-- SP_USER_FIND_BY_ID — read user by GUID for refresh-token flow (M14.5+).
-- Companion to SP_USER_FIND_BY_USERNAME; same column shape (profile +
-- permissions rows, discriminated by row_kind) so MssqlUserRepository
-- ::row_to_user can read either without modification.
--
-- ponytail: varchar(50) GUID param per project convention — DB GUID
-- columns are stored as varchar/nvarchar (implicit-cast at WHERE).
-- LTRIM/RTRIM/ISNULL/IF-empty guards mirror SP_USER_FIND_BY_USERNAME
-- pattern. Ceiling: SP_-prefixed read SP family only — API_-prefixed
-- SPs keep UNIQUEIDENTIFIER params (caller binds natively).

IF OBJECT_ID('dbo.SP_USER_FIND_BY_ID', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.SP_USER_FIND_BY_ID AS BEGIN SET NOCOUNT ON; END');
GO

ALTER   PROCEDURE dbo.SP_USER_FIND_BY_ID
    @p_user_guid varchar(50)
AS
BEGIN
    SET NOCOUNT ON;

    DECLARE @user_guid varchar(50);

    SELECT TOP 1
        @user_guid = un.user_username_user_guid
    FROM dbo.user_username AS un
    INNER JOIN dbo.[user] AS u
        ON u.user_guid = un.user_username_user_guid
    WHERE un.user_username_guid = @p_user_guid
      AND un.user_username_status = 1
      AND u.user_status = 1;

    IF @user_guid IS NULL
    BEGIN
        RETURN;
    END;

    ;WITH active_roles AS (
        SELECT
            uur.user_user_role_user_guid,
            r.user_role_guid,
            r.user_role_code
        FROM dbo.user_user_role AS uur
        INNER JOIN dbo.user_role AS r
            ON r.user_role_guid = uur.user_user_role_role_guid
        WHERE uur.user_user_role_user_guid = @user_guid
          AND uur.user_user_role_status = 1
          AND r.user_role_status = 1
          AND (
                uur.user_user_role_expire_at IS NULL
                OR uur.user_user_role_expire_at > SYSUTCDATETIME()
              )
    ),
    role_permissions AS (
        SELECT DISTINCT
            p.user_permission_guid,
            p.user_permission_code
        FROM active_roles AS ar
        INNER JOIN dbo.user_role_permission AS rp
            ON rp.user_role_permission_user_role_guid = ar.user_role_guid
        INNER JOIN dbo.user_permission AS p
            ON p.user_permission_guid = rp.user_role_permission_user_permission_guid
        WHERE rp.user_role_permission_status = 1
          AND p.user_permission_status = 1
    ),
    override_allow AS (
        SELECT DISTINCT
            p.user_permission_guid,
            p.user_permission_code
        FROM dbo.user_permission_override AS o
        INNER JOIN dbo.user_permission AS p
            ON p.user_permission_guid = o.user_permission_override_permission_guid
        WHERE o.user_permission_override_user_guid = @user_guid
          AND o.user_permission_override_effect = 'allow'
          AND o.user_permission_override_status = 1
          AND p.user_permission_status = 1
          AND (
                o.user_permission_override_expire_at IS NULL
                OR o.user_permission_override_expire_at > SYSUTCDATETIME()
              )
    ),
    override_deny AS (
        SELECT DISTINCT
            p.user_permission_guid,
            p.user_permission_code
        FROM dbo.user_permission_override AS o
        INNER JOIN dbo.user_permission AS p
            ON p.user_permission_guid = o.user_permission_override_permission_guid
        WHERE o.user_permission_override_user_guid = @user_guid
          AND o.user_permission_override_effect = 'deny'
          AND o.user_permission_override_status = 1
          AND p.user_permission_status = 1
          AND (
                o.user_permission_override_expire_at IS NULL
                OR o.user_permission_override_expire_at > SYSUTCDATETIME()
              )
    ),
    combined_allow AS (
        SELECT * FROM role_permissions
        UNION
        SELECT * FROM override_allow
    ),
    effective_permissions AS (
        SELECT DISTINCT
            ca.user_permission_guid,
            ca.user_permission_code
        FROM combined_allow AS ca
        WHERE NOT EXISTS (
            SELECT 1
            FROM override_deny AS od
            WHERE od.user_permission_guid = ca.user_permission_guid
        )
    )
    SELECT
        CAST(NULL AS nvarchar(max)) AS permission_codes,
        CAST((
            SELECT STRING_AGG(ar.user_role_code, ',')
            FROM active_roles AS ar
        ) AS nvarchar(max)) AS role_codes,

        CAST('profile' AS varchar(50)) AS row_kind,

         un.user_username_guid AS user_guid,
        un.user_username_username AS user_username_username,
        un.user_username_password_hash AS user_password,
        u.user_first_name AS user_first_name,
        u.user_last_name AS user_last_name,
        u.user_status AS user_status,
        un.user_username_create_at AS user_username_create_at,
        ISNULL(un.user_username_update_at, un.user_username_create_at) AS user_username_update_at
    FROM dbo.user_username AS un
    INNER JOIN dbo.[user] AS u
        ON u.user_guid = un.user_username_user_guid
    WHERE u.user_guid = @user_guid
      AND un.user_username_guid = @p_user_guid

    UNION ALL

    SELECT

       CAST(ISNULL((
          SELECT STRING_AGG(ep.user_permission_code, ',')
          FROM effective_permissions AS ep
      ), N'') AS nvarchar(max)) AS permission_codes,
        CAST(ISNULL((
          SELECT STRING_AGG(ar.user_role_code, ',')
          FROM active_roles AS ar
      ), N'') AS nvarchar(max)) AS role_codes,


        CAST('permissions' AS varchar(50)) AS row_kind,

        un.user_username_guid AS user_guid,
        un.user_username_username AS user_username_username,
        un.user_username_password_hash AS user_password,
        u.user_first_name AS user_first_name,
        u.user_last_name AS user_last_name,
        u.user_status AS user_status,
        un.user_username_create_at AS user_username_create_at,
        ISNULL(un.user_username_update_at, un.user_username_create_at) AS user_username_update_at
    FROM dbo.user_username AS un
    INNER JOIN dbo.[user] AS u
        ON u.user_guid = un.user_username_user_guid
    WHERE u.user_guid = @user_guid
      AND un.user_username_guid = @p_user_guid;
END;
END;
GO
