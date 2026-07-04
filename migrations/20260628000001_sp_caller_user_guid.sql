


































































IF OBJECT_ID('dbo.SP_PERMISSION_USER_LIST_V2', 'P') IS NOT NULL
BEGIN
    DROP PROCEDURE dbo.SP_PERMISSION_USER_LIST_V2;
END;
GO




























IF OBJECT_ID('dbo.SP_PERMISSION_USER_LIST', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.SP_PERMISSION_USER_LIST AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.SP_PERMISSION_USER_LIST
    
    
    
    
    
    @p_user_guid varchar(36) = NULL
AS
BEGIN
    SET NOCOUNT ON;

    DECLARE @p_user_guid_uid uniqueidentifier =
        CASE WHEN @p_user_guid IS NULL OR LTRIM(RTRIM(@p_user_guid)) = ''
             THEN NULL
             ELSE CAST(@p_user_guid AS uniqueidentifier) END;

    
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

    ;WITH user_active_roles AS (
        SELECT
            u.user_guid,
            
            STRING_AGG(r.user_role_code, ',')
                WITHIN GROUP (ORDER BY r.user_role_code) AS role_codes_csv,
            STRING_AGG(r.user_role_name, ',')
                WITHIN GROUP (ORDER BY r.user_role_code) AS role_names_csv,
            
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










IF OBJECT_ID('dbo.SP_PERMISSION_USER_DETAIL_FIND_BY_GUID', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.SP_PERMISSION_USER_DETAIL_FIND_BY_GUID AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.SP_PERMISSION_USER_DETAIL_FIND_BY_GUID
    @p_user_guid         varchar(36),            
    @p_caller_user_guid  varchar(36) = NULL       
AS
BEGIN
    SET NOCOUNT ON;

    
    DECLARE @user_guid_uid uniqueidentifier =
        TRY_CAST(NULLIF(LTRIM(RTRIM(@p_user_guid)), '') AS uniqueidentifier);
    DECLARE @caller_guid_uid uniqueidentifier =
        TRY_CAST(NULLIF(LTRIM(RTRIM(@p_caller_user_guid)), '') AS uniqueidentifier);

    
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






IF OBJECT_ID('dbo.SP_USER_GROUP_ROLE', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.SP_USER_GROUP_ROLE AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.SP_USER_GROUP_ROLE
    @p_mode      NVARCHAR(32),       
    @p_user_guid varchar(36) = NULL  
AS
BEGIN
    SET NOCOUNT ON;

    DECLARE @p_user_guid_uid uniqueidentifier =
        TRY_CAST(NULLIF(LTRIM(RTRIM(@p_user_guid)), '') AS uniqueidentifier);

    
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
