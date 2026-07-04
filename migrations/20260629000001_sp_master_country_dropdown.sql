









































IF OBJECT_ID('dbo.SP_MASTER_COUNTRY_DROPDOWN_GET', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.SP_MASTER_COUNTRY_DROPDOWN_GET AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.SP_MASTER_COUNTRY_DROPDOWN_GET
    @p_keyword                 nvarchar(255) = NULL,
    @p_master_country_status   int          = 1
AS
BEGIN
    SET NOCOUNT ON;

    SELECT
        COALESCE(mc.master_country_guid, '') AS value,
        COALESCE(mc.master_country_name, '') AS label
    FROM dbo.master_country AS mc
    WHERE mc.master_country_status <> 3
      AND (
            @p_master_country_status IS NULL
            OR mc.master_country_status = @p_master_country_status
          )
      AND (
            @p_keyword IS NULL
            OR LTRIM(RTRIM(@p_keyword)) = ''
            OR mc.master_country_name LIKE '%' + @p_keyword + '%'
            OR mc.master_country_code LIKE '%' + @p_keyword + '%'
          )
    ORDER BY
        mc.master_country_name ASC,
        mc.master_country_code ASC;
END;
GO
