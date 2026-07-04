

















































IF OBJECT_ID('dbo.SP_AUTOCOMPLETE_USER_DEPARTMENT_GET', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.SP_AUTOCOMPLETE_USER_DEPARTMENT_GET AS BEGIN SET NOCOUNT ON; END');
GO

CREATE OR ALTER PROCEDURE dbo.SP_AUTOCOMPLETE_USER_DEPARTMENT_GET
    @p_keyword nvarchar(255) = NULL,
    @p_take int = 20
AS
BEGIN
    SET NOCOUNT ON;

    SET @p_keyword = LTRIM(RTRIM(ISNULL(@p_keyword, N'')));

    IF @p_take IS NULL OR @p_take <= 0
        SET @p_take = 20;

    IF @p_take > 100
        SET @p_take = 100;

    SELECT TOP (@p_take)
        ud.user_department_guid AS value,
        ud.user_department_name AS label,

        ud.user_department_guid,
        ud.user_department_code,
        ud.user_department_name,
        ud.user_department_status
    FROM dbo.user_department AS ud
    WHERE ud.user_department_status = 1
      AND (
            @p_keyword = N''
            OR ud.user_department_name LIKE @p_keyword + N'%'
            OR ud.user_department_code LIKE @p_keyword + '%'
          )
    ORDER BY
        ud.user_department_name ASC,
        ud.user_department_code ASC
    OPTION (RECOMPILE);
END;
GO
