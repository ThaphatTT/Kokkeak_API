






IF OBJECT_ID('dbo.translation_override', 'U') IS NULL
CREATE TABLE [translation_override] (
    translation_override_guid    UNIQUEIDENTIFIER NOT NULL PRIMARY KEY,
    translation_override_locale   NVARCHAR(8)      NOT NULL,
    translation_override_key      NVARCHAR(255)    NOT NULL,
    translation_override_value    NVARCHAR(MAX)    NOT NULL,
    translation_override_updated_by UNIQUEIDENTIFIER NULL REFERENCES [user](user_guid),
    translation_override_updated_at DATETIME2(7)    NOT NULL DEFAULT SYSUTCDATETIME(),
    translation_override_create_at  DATETIME2(7)    NOT NULL DEFAULT SYSUTCDATETIME(),
    CONSTRAINT uq_translation_override_locale_key UNIQUE (translation_override_locale, translation_override_key)
);
GO

IF NOT EXISTS (SELECT 1 FROM sys.indexes WHERE name = 'ix_translation_override_locale')
CREATE INDEX ix_translation_override_locale ON [translation_override](translation_override_locale);
GO




IF OBJECT_ID('dbo.API_TRANSLATION_GET', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.API_TRANSLATION_GET AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.API_TRANSLATION_GET
    @p_locale NVARCHAR(8),
    @p_key    NVARCHAR(255)
AS
BEGIN
    SET NOCOUNT ON;

    SELECT
        translation_override_value AS value
    FROM [translation_override]
    WHERE translation_override_locale = @p_locale
      AND translation_override_key    = @p_key;
END;
GO




IF OBJECT_ID('dbo.API_TRANSLATION_PUT', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.API_TRANSLATION_PUT AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.API_TRANSLATION_PUT
    @p_locale    NVARCHAR(8),
    @p_key       NVARCHAR(255),
    @p_value     NVARCHAR(MAX),
    @p_user_guid UNIQUEIDENTIFIER
AS
BEGIN
    SET NOCOUNT ON;

    IF EXISTS (
        SELECT 1 FROM [translation_override]
        WHERE translation_override_locale = @p_locale
          AND translation_override_key    = @p_key
    )
    BEGIN
        UPDATE [translation_override]
        SET translation_override_value     = @p_value,
            translation_override_updated_by = @p_user_guid,
            translation_override_updated_at = SYSUTCDATETIME()
        WHERE translation_override_locale = @p_locale
          AND translation_override_key    = @p_key;
    END
    ELSE
    BEGIN
        INSERT INTO [translation_override] (
            translation_override_guid, translation_override_locale,
            translation_override_key, translation_override_value,
            translation_override_updated_by
        ) VALUES (
            NEWID(), @p_locale, @p_key, @p_value, @p_user_guid
        );
    END;

    SELECT CAST(NULL AS UNIQUEIDENTIFIER) AS id, 0 AS error_code, '' AS error_message;
END;
GO




IF OBJECT_ID('dbo.API_TRANSLATION_LIST_BY_LOCALE', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.API_TRANSLATION_LIST_BY_LOCALE AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.API_TRANSLATION_LIST_BY_LOCALE
    @p_locale NVARCHAR(8)
AS
BEGIN
    SET NOCOUNT ON;

    SELECT
        translation_override_key   AS [key],
        translation_override_value AS value
    FROM [translation_override]
    WHERE translation_override_locale = @p_locale
    ORDER BY translation_override_key;
END;
GO
