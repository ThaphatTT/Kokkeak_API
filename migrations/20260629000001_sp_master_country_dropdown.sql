-- ============================================================================
-- M20: Master-data dropdown read stored procedure (country first).
-- -----------------------------------------------------------------------------
-- Backs `GET /api/v1/master/countries` — a centralized lookup endpoint
-- consumed by mobile (customer/technician apps), the customer-facing
-- web frontend, and the admin web console. Same route, same wire shape
-- everywhere; only the client UI differs.
--
-- Filter semantics (both optional):
--
--   @p_keyword                 nvarchar(255) = NULL
--     Free-text filter against `master_country_name` and
--     `master_country_code` (case-sensitive LIKE because the SP
--     doesn't LOWER the column). NULL / blank = no filter
--     (return all matching rows).
--
--   @p_master_country_status   int = 1
--     `master_country_status` to filter on. Default 1 (active).
--     Caller may pass 0 (inactive) or 1 (active). The status=3
--     (deleted) bucket is hard-excluded in the WHERE clause.
--     To get all non-deleted statuses regardless of active/inactive,
--     the caller may bind SQL NULL (skips this filter via the
--     `OR @p_master_country_status IS NULL` branch).
--
-- Result columns (one row per country):
--   value   nvarchar  -- `master_country_guid` (string, project convention
--                       from M19: GUIDs into `dbo.SP_*` arrive as text,
--                       not native UNIQUEIDENTIFIER)
--   label   nvarchar  -- `master_country_name` (localised display label)
--
-- Hard-excludes:
--   - status = 3 (deleted) — never returned regardless of @p_*
--   - duplicate rows — currently impossible (master_country has no
--     dedup-key on the design; if a future migration adds one,
--     add SELECT DISTINCT here)
--
-- Admin gate: NOT applied. Country dropdown is shared reference data
-- consumed by every authenticated role (mobile, customer, admin).
-- If a future master-data SP is admin-only (e.g. internal taxonomy
-- editing), add `@p_user_guid` admin gate per the M19 contract.
-- ============================================================================

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
