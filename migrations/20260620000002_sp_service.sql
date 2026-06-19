-- ============================================================================
-- KOKKAK NEW_DB v2 - Stored Procedures: Service Catalog
--
-- Reference: NEW_DB.txt `category_job_service_sub` is the leaf level the mobile
-- client renders. We expose a flat list keyed by guid + a top-N "active" view.
-- ============================================================================

-- ----------------------------------------------------------------------------
-- API_SERVICE_LIST_ACTIVE
-- Returns all currently-published service categories (master / sub / icon /
-- price / warranty). Single result set for the mobile home screen.
-- ----------------------------------------------------------------------------
IF OBJECT_ID('dbo.API_SERVICE_LIST_ACTIVE', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.API_SERVICE_LIST_ACTIVE AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.API_SERVICE_LIST_ACTIVE
    @p_lang_code NVARCHAR(8) = N'th'  -- for future translation tables
AS
BEGIN
    SET NOCOUNT ON;

    SELECT
        cjs.category_job_service_sub_guid           AS id,
        cjs.category_job_service_sub_category_main_guid AS category_main_id,
        cjs.category_job_service_sub_category_job_guid  AS category_job_id,
        cjs.category_job_service_sub_name_th         AS name_th,
        cjs.category_job_service_sub_name_en         AS name_en,
        cjs.category_job_service_sub_name_lo         AS name_lo,
        cjs.category_job_service_sub_icon_style      AS icon_style,
        cjs.category_job_service_sub_icon_line       AS icon_line,
        cjs.category_job_service_sub_img_path        AS img_path,
        cjs.category_job_service_sub_status          AS status,
        cjs.category_job_service_sub_priority        AS priority,
        cjs.category_job_service_sub_create_at       AS created_at
    FROM [category_job_service_sub] cjs
    WHERE cjs.category_job_service_sub_status = 1
    ORDER BY cjs.category_job_service_sub_priority DESC,
             cjs.category_job_service_sub_create_at DESC;
END;
GO

-- ----------------------------------------------------------------------------
-- API_SERVICE_GET
-- Single service lookup by id.
-- ----------------------------------------------------------------------------
IF OBJECT_ID('dbo.API_SERVICE_GET', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.API_SERVICE_GET AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.API_SERVICE_GET
    @p_service_id UNIQUEIDENTIFIER,
    @p_result     INT OUTPUT
AS
BEGIN
    SET NOCOUNT ON;

    SELECT
        cjs.category_job_service_sub_guid           AS id,
        cjs.category_job_service_sub_category_main_guid AS category_main_id,
        cjs.category_job_service_sub_category_job_guid  AS category_job_id,
        cjs.category_job_service_sub_name_th         AS name_th,
        cjs.category_job_service_sub_name_en         AS name_en,
        cjs.category_job_service_sub_name_lo         AS name_lo,
        cjs.category_job_service_sub_icon_style      AS icon_style,
        cjs.category_job_service_sub_icon_line       AS icon_line,
        cjs.category_job_service_sub_img_path        AS img_path,
        cjs.category_job_service_sub_status          AS status,
        cjs.category_job_service_sub_priority        AS priority,
        cjs.category_job_service_sub_create_at       AS created_at
    FROM [category_job_service_sub] cjs
    WHERE cjs.category_job_service_sub_guid = @p_service_id;

    IF @@ROWCOUNT = 0
        SET @p_result = 1;
    ELSE
        SET @p_result = 0;
END;
GO

-- ----------------------------------------------------------------------------
-- API_SERVICE_MAIN_LIST
-- Top-level category list (e.g. "Plumbing", "Electrical").
-- ----------------------------------------------------------------------------
IF OBJECT_ID('dbo.API_SERVICE_MAIN_LIST', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.API_SERVICE_MAIN_LIST AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.API_SERVICE_MAIN_LIST
AS
BEGIN
    SET NOCOUNT ON;

    SELECT
        cjm.category_job_main_guid           AS id,
        cjm.category_job_main_name            AS name,
        cjm.category_job_main_icon_style      AS icon_style,
        cjm.category_job_main_icon_line       AS icon_line,
        cjm.category_job_main_img_path        AS img_path,
        cjm.category_job_main_priority        AS priority,
        cjm.category_job_main_status          AS status
    FROM [category_job_main] cjm
    WHERE cjm.category_job_main_status = 1
    ORDER BY cjm.category_job_main_priority DESC;
END;
GO
