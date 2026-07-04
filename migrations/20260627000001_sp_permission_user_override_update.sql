









































IF OBJECT_ID('dbo.SP_PERMISSION_USER_OVERRIDE_UPDATE', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.SP_PERMISSION_USER_OVERRIDE_UPDATE AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.SP_PERMISSION_USER_OVERRIDE_UPDATE
    @p_user_permission_override_user_guid varchar(50),
    @p_user_permission_override_permission_guid varchar(50),
    @p_user_permission_override_effect varchar(10),

    @p_user_permission_override_reason nvarchar(max) = NULL,
    @p_user_permission_override_assigned_by varchar(50) = NULL,
    @p_user_permission_override_status int = 1,

    @p_create_by varchar(50) = NULL,
    @p_update_by varchar(50) = NULL
AS
BEGIN
    SET NOCOUNT ON;

    BEGIN TRY
        BEGIN TRANSACTION;

        DECLARE @now datetime2(7) = SYSUTCDATETIME();
        DECLARE @existing_guid varchar(50);

        SET @p_user_permission_override_effect =
            LOWER(LTRIM(RTRIM(@p_user_permission_override_effect)));

        IF @p_create_by IS NULL OR LTRIM(RTRIM(@p_create_by)) = ''
            SET @p_create_by = 'system';

        IF @p_update_by IS NULL OR LTRIM(RTRIM(@p_update_by)) = ''
            SET @p_update_by = @p_create_by;

        IF @p_user_permission_override_assigned_by IS NULL
           OR LTRIM(RTRIM(@p_user_permission_override_assigned_by)) = ''
            SET @p_user_permission_override_assigned_by = @p_update_by;

        
        IF @p_user_permission_override_effect NOT IN ('allow', 'deny')
        BEGIN
            ROLLBACK TRANSACTION;

            SELECT
                CAST(0 AS bit) AS success,
                'INVALID_EFFECT' AS code,
                'user_permission_override_effect must be allow or deny' AS message;

            RETURN;
        END;

        
        IF @p_user_permission_override_status NOT IN (0, 1)
        BEGIN
            ROLLBACK TRANSACTION;

            SELECT
                CAST(0 AS bit) AS success,
                'INVALID_STATUS' AS code,
                'user_permission_override_status must be 0 or 1' AS message;

            RETURN;
        END;

        
        IF NOT EXISTS (
            SELECT 1
            FROM dbo.[user]
            WHERE user_guid = @p_user_permission_override_user_guid
              AND user_status <> 3
        )
        BEGIN
            ROLLBACK TRANSACTION;

            SELECT
                CAST(0 AS bit) AS success,
                'USER_NOT_FOUND' AS code,
                'user_permission_override_user_guid not found' AS message;

            RETURN;
        END;

        
        IF NOT EXISTS (
            SELECT 1
            FROM dbo.user_permission
            WHERE user_permission_guid = @p_user_permission_override_permission_guid
              AND user_permission_status <> 3
        )
        BEGIN
            ROLLBACK TRANSACTION;

            SELECT
                CAST(0 AS bit) AS success,
                'PERMISSION_NOT_FOUND' AS code,
                'user_permission_override_permission_guid not found' AS message;

            RETURN;
        END;

        
        SELECT TOP 1
            @existing_guid = user_permission_override_guid
        FROM dbo.user_permission_override WITH (UPDLOCK, HOLDLOCK)
        WHERE user_permission_override_user_guid = @p_user_permission_override_user_guid
          AND user_permission_override_permission_guid = @p_user_permission_override_permission_guid;

        
        IF @existing_guid IS NOT NULL
        BEGIN
            UPDATE dbo.user_permission_override
            SET
                user_permission_override_effect = @p_user_permission_override_effect,
                user_permission_override_reason = @p_user_permission_override_reason,
                user_permission_override_assigned_by = @p_user_permission_override_assigned_by,
                user_permission_override_assigned_at = @now,
                user_permission_override_status = @p_user_permission_override_status,
                user_permission_override_update_at = @now,
                user_permission_override_update_by = @p_update_by
            WHERE user_permission_override_guid = @existing_guid;

            COMMIT TRANSACTION;

            SELECT
                CAST(1 AS bit) AS success,
                'UPDATED' AS code,
                'User permission override updated' AS message,
                @existing_guid AS user_permission_override_guid,
                @p_user_permission_override_user_guid AS user_permission_override_user_guid,
                @p_user_permission_override_permission_guid AS user_permission_override_permission_guid,
                @p_user_permission_override_effect AS user_permission_override_effect,
                @p_user_permission_override_status AS user_permission_override_status;

            RETURN;
        END;

        
        SET @existing_guid = CONVERT(varchar(50), NEWID());

        INSERT INTO dbo.user_permission_override (
            user_permission_override_guid,
            user_permission_override_user_guid,
            user_permission_override_permission_guid,
            user_permission_override_effect,
            user_permission_override_reason,
            user_permission_override_assigned_by,
            user_permission_override_assigned_at,
            user_permission_override_expire_at,
            user_permission_override_status,
            user_permission_override_create_at,
            user_permission_override_create_by,
            user_permission_override_update_at,
            user_permission_override_update_by
        )
        VALUES (
            @existing_guid,
            @p_user_permission_override_user_guid,
            @p_user_permission_override_permission_guid,
            @p_user_permission_override_effect,
            @p_user_permission_override_reason,
            @p_user_permission_override_assigned_by,
            @now,
            '',
            @p_user_permission_override_status,
            @now,
            @p_create_by,
            NULL,
            NULL
        );

        COMMIT TRANSACTION;

        SELECT
            CAST(1 AS bit) AS success,
            'CREATED' AS code,
            'User permission override created' AS message,
            @existing_guid AS user_permission_override_guid,
            @p_user_permission_override_user_guid AS user_permission_override_user_guid,
            @p_user_permission_override_permission_guid AS user_permission_override_permission_guid,
            @p_user_permission_override_effect AS user_permission_override_effect,
            @p_user_permission_override_status AS user_permission_override_status;

    END TRY
    BEGIN CATCH
        IF @@TRANCOUNT > 0
            ROLLBACK TRANSACTION;

        SELECT
            CAST(0 AS bit) AS success,
            'ERROR' AS code,
            ERROR_MESSAGE() AS message,
            ERROR_NUMBER() AS error_number,
            ERROR_LINE() AS error_line;

        THROW;
    END CATCH;
END;
GO
