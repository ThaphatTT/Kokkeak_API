-- ============================================================================
-- KOKKAK NEW DB v2 - Stored Procedures: Payment
-- ============================================================================

-- ----------------------------------------------------------------------------
-- API_PAYMENT_CREATE
-- ----------------------------------------------------------------------------
IF OBJECT_ID('dbo.API_PAYMENT_CREATE', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.API_PAYMENT_CREATE AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.API_PAYMENT_CREATE
    @p_order_guid       UNIQUEIDENTIFIER,
    @p_customer_guid    UNIQUEIDENTIFIER,
    @p_amount           DECIMAL(18, 2),
    @p_method_code      NVARCHAR(32)
AS
BEGIN
    SET NOCOUNT ON;
    DECLARE @p UNIQUEIDENTIFIER = NEWID();

    INSERT INTO [order_service_payment] (
        order_service_payment_guid, order_service_payment_order_guid,
        order_service_payment_customer_guid, order_service_payment_amount,
        order_service_payment_method, order_service_payment_status,
        order_service_payment_create_at, order_service_payment_create_by,
        order_service_payment_update_at, order_service_payment_update_by
    ) VALUES (
        @p, @p_order_guid,
        @p_customer_guid, @p_amount,
        @p_method_code, 1,
        SYSUTCDATETIME(), @p_customer_guid,
        SYSUTCDATETIME(), @p_customer_guid
    );

    SELECT @p AS id, 0 AS error_code, '' AS error_message;
END;
GO

-- ----------------------------------------------------------------------------
-- API_PAYMENT_CONFIRM
-- Atomic status flip 1 (pending) -> 2 (paid).
-- ----------------------------------------------------------------------------
IF OBJECT_ID('dbo.API_PAYMENT_CONFIRM', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.API_PAYMENT_CONFIRM AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.API_PAYMENT_CONFIRM
    @p_payment_guid UNIQUEIDENTIFIER,
    @p_external_ref NVARCHAR(255)
AS
BEGIN
    SET NOCOUNT ON;

    UPDATE [order_service_payment]
    SET order_service_payment_status      = 2,
        order_service_payment_external_ref = @p_external_ref,
        order_service_payment_paid_at      = SYSUTCDATETIME(),
        order_service_payment_update_at    = SYSUTCDATETIME()
    WHERE order_service_payment_guid = @p_payment_guid
      AND order_service_payment_status = 1;

    IF @@ROWCOUNT = 0
        SELECT @p_payment_guid AS id, 1 AS error_code, N'payment not pending' AS error_message;
    ELSE
        SELECT @p_payment_guid AS id, 0 AS error_code, '' AS error_message;
END;
GO

-- ----------------------------------------------------------------------------
-- API_PAYMENT_CALC_COMMISSION
-- Returns commission rate + commission amount + net to technician.
-- 3 columns + error_code/message for uniform shape.
-- ----------------------------------------------------------------------------
IF OBJECT_ID('dbo.API_PAYMENT_CALC_COMMISSION', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.API_PAYMENT_CALC_COMMISSION AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.API_PAYMENT_CALC_COMMISSION
    @p_gross_amount DECIMAL(18, 2)
AS
BEGIN
    SET NOCOUNT ON;
    DECLARE @rate DECIMAL(5, 2);

    SELECT @rate = ISNULL(system_vat_percentage, 0)
    FROM [system_vat]
    WHERE system_vat_status = 1;

    SELECT
        @p_gross_amount AS gross_amount,
        @rate            AS commission_rate,
        ROUND(@p_gross_amount * @rate / 100, 2) AS commission_amount,
        @p_gross_amount - ROUND(@p_gross_amount * @rate / 100, 2) AS net_to_tech,
        0 AS error_code,
        '' AS error_message;
END;
GO

-- ----------------------------------------------------------------------------
-- API_PAYMENT_LIST_BY_TECHNICIAN
-- ----------------------------------------------------------------------------
IF OBJECT_ID('dbo.API_PAYMENT_LIST_BY_TECHNICIAN', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.API_PAYMENT_LIST_BY_TECHNICIAN AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.API_PAYMENT_LIST_BY_TECHNICIAN
    @p_technician_guid UNIQUEIDENTIFIER,
    @p_limit            INT = 50,
    @p_offset           INT = 0
AS
BEGIN
    SET NOCOUNT ON;

    SELECT
        p.order_service_payment_guid         AS id,
        p.order_service_payment_order_guid    AS order_id,
        p.order_service_payment_amount        AS amount,
        p.order_service_payment_method        AS method,
        p.order_service_payment_status        AS status,
        p.order_service_payment_external_ref  AS external_ref,
        p.order_service_payment_paid_at       AS paid_at,
        p.order_service_payment_create_at     AS created_at
    FROM [order_service_payment] p
    INNER JOIN [order_service_header] h
        ON h.order_service_header_guid = p.order_service_payment_order_guid
    INNER JOIN [order_service_assignment] a
        ON a.order_service_assignment_header_guid = h.order_service_header_guid
    WHERE a.order_service_assignment_technician_guid = @p_technician_guid
    ORDER BY p.order_service_payment_create_at DESC
    OFFSET @p_offset ROWS
    FETCH NEXT @p_limit ROWS ONLY;
END;
GO
