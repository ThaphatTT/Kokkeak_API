ALTER   FUNCTION dbo.FN_SECURITY_USER_HAS_PERMISSION
(
    @p_user_username_guid varchar(50),
    @p_permission_code varchar(150)
)
RETURNS TABLE
AS
RETURN
(
    WITH resolved_user AS (
        SELECT
            user_guid
        FROM dbo.FN_SECURITY_RESOLVE_USER_GUID_BY_USERNAME_GUID(@p_user_username_guid)
    ),
    active_roles AS (
        SELECT
            r.user_role_guid
        FROM resolved_user AS ru
        INNER JOIN dbo.user_user_role AS uur
            ON uur.user_user_role_user_guid = ru.user_guid
        INNER JOIN dbo.user_role AS r
            ON r.user_role_guid = uur.user_user_role_role_guid
        WHERE uur.user_user_role_status = 1
          AND r.user_role_status = 1
          AND (
                uur.user_user_role_expire_at IS NULL
                OR uur.user_user_role_expire_at > SYSUTCDATETIME()
              )
    ),
    role_allow AS (
        SELECT DISTINCT
            p.user_permission_guid
        FROM active_roles AS ar
        INNER JOIN dbo.user_role_permission AS rp
            ON rp.user_role_permission_user_role_guid = ar.user_role_guid
        INNER JOIN dbo.user_permission AS p
            ON p.user_permission_guid = rp.user_role_permission_user_permission_guid
        WHERE rp.user_role_permission_status = 1
          AND p.user_permission_status = 1
          AND p.user_permission_code = @p_permission_code
    ),
    override_allow AS (
        SELECT DISTINCT
            p.user_permission_guid
        FROM resolved_user AS ru
        INNER JOIN dbo.user_permission_override AS o
            ON o.user_permission_override_user_guid = ru.user_guid
        INNER JOIN dbo.user_permission AS p
            ON p.user_permission_guid = o.user_permission_override_permission_guid
        WHERE o.user_permission_override_effect = 'allow'
          AND o.user_permission_override_status = 1
          AND p.user_permission_status = 1
          AND p.user_permission_code = @p_permission_code
          AND (
                o.user_permission_override_expire_at IS NULL
                OR o.user_permission_override_expire_at > SYSUTCDATETIME()
              )
    ),
    override_deny AS (
        SELECT DISTINCT
            p.user_permission_guid
        FROM resolved_user AS ru
        INNER JOIN dbo.user_permission_override AS o
            ON o.user_permission_override_user_guid = ru.user_guid
        INNER JOIN dbo.user_permission AS p
            ON p.user_permission_guid = o.user_permission_override_permission_guid
        WHERE o.user_permission_override_effect = 'deny'
          AND o.user_permission_override_status = 1
          AND p.user_permission_status = 1
          AND p.user_permission_code = @p_permission_code
          AND (
                o.user_permission_override_expire_at IS NULL
                OR o.user_permission_override_expire_at > SYSUTCDATETIME()
              )
    ),
    combined_allow AS (
        SELECT user_permission_guid FROM role_allow
        UNION
        SELECT user_permission_guid FROM override_allow
    )
    SELECT
        CAST(
            CASE
                WHEN NOT EXISTS (SELECT 1 FROM resolved_user) THEN 0
                WHEN EXISTS (SELECT 1 FROM override_deny) THEN 0
                WHEN EXISTS (SELECT 1 FROM combined_allow) THEN 1
                ELSE 0
            END
        AS bit) AS is_allowed
);
