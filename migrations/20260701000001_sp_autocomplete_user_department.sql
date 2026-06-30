-- ============================================================================
-- M20-b: Autocomplete read stored procedure (user_department).
-- -----------------------------------------------------------------------------
-- Backs `GET /api/v1/master/user-departments` — a centralized typeahead
-- endpoint consumed by admin web (user → department assignment) and
-- customer mobile (technician department filter). Same /api/v1/master/*
-- prefix as the dropdown endpoints, but a **different SP family** because
-- the filter semantics differ:
--
--   SP_MASTER_*_DROPDOWN_GET   — full bounded list, filter by status
--   SP_AUTOCOMPLETE_*_GET      — typeahead, bounded by @p_take, prefix match
--
-- Filter semantics (both optional):
--
--   @p_keyword   nvarchar(255) = NULL
--     Free-text filter. SP trims + coerces NULL/blank to ''. Empty
--     keyword = no filter (return top @p_take rows). Non-blank
--     matches against `user_department_name` (prefix) and
--     `user_department_code` (prefix) — typeahead UX, not substring.
--
--   @p_take      int = 20
--     Maximum rows to return. SP clamps to [1, 100]:
--       - NULL or <= 0 → 20 (default)
--       - > 100        → 100 (cap)
--     Caller never sees a > 100 row count, and the Rust layer
--     trusts this default + cap (no duplicate logic on the
--     client side).
--
-- Result columns (one row per department):
--   value                       nvarchar  -- user_department_guid (string,
--                                            M19 convention: SP_-prefixed
--                                            GUIDs arrive as varchar(36))
--   label                       nvarchar  -- user_department_name
--                                            (localised display label)
--   user_department_guid        nvarchar  -- (extras: full row, currently
--   user_department_code        nvarchar  --  unused by the autocomplete
--   user_department_name        nvarchar  --  wire shape — kept on the SP
--   user_department_status      int       --  so a future "detail on
--                                            select" endpoint can swap
--                                            SPs without a DB migration)
--
-- Hard-excludes:
--   - status = 3 (deleted) — never returned regardless of @p_*
--
-- Admin gate: NOT applied. Department list is shared reference data
-- consumed by every authenticated role (mobile, customer, admin).
-- If a future autocomplete SP becomes admin-only, add `@p_user_guid`
-- admin gate per the M19 contract.
-- ============================================================================

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
