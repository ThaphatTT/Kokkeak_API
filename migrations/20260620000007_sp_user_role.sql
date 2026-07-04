
































IF OBJECT_ID('[user_permission]', 'U') IS NULL
CREATE TABLE [user_permission] (
    user_permission_guid       UNIQUEIDENTIFIER NOT NULL PRIMARY KEY,
    user_permission_code       NVARCHAR(64)     NOT NULL,
    user_permission_name       NVARCHAR(100)    NOT NULL,
    user_permission_module     NVARCHAR(32)     NULL,    
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

























IF OBJECT_ID('dbo.SP_USER_GROUP_ROLE', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.SP_USER_GROUP_ROLE AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.SP_USER_GROUP_ROLE
    @p_mode NVARCHAR(32)   
AS
BEGIN
    SET NOCOUNT ON;

    
    
    
    
    
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
