


























IF OBJECT_ID('dbo.SP_PERMISSION_USER_LIST', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.SP_PERMISSION_USER_LIST AS BEGIN SET NOCOUNT ON; END');
GO


























IF OBJECT_ID('dbo.SP_PERMISSION_USER_DETAIL_FIND_BY_GUID', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.SP_PERMISSION_USER_DETAIL_FIND_BY_GUID AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.SP_PERMISSION_USER_DETAIL_FIND_BY_GUID
    @p_user_guid UNIQUEIDENTIFIER
AS
BEGIN
    SET NOCOUNT ON;

    
    
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
