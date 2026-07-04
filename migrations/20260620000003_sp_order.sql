













IF OBJECT_ID('dbo.API_ORDER_CREATE', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.API_ORDER_CREATE AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.API_ORDER_CREATE
    @p_customer_guid    UNIQUEIDENTIFIER,
    @p_service_id       UNIQUEIDENTIFIER,
    @p_address          NVARCHAR(MAX),
    @p_description      NVARCHAR(MAX) = NULL,
    @p_latitude         DECIMAL(9, 6) = NULL,
    @p_longitude        DECIMAL(9, 6) = NULL,
    @p_total_amount     DECIMAL(18, 2) = 0
AS
BEGIN
    SET NOCOUNT ON;
    DECLARE @new_guid UNIQUEIDENTIFIER = NEWID();

    BEGIN TRAN;

    INSERT INTO [order_service_header] (
        order_service_header_guid, order_service_header_customer_guid,
        order_service_header_status, order_service_header_address,
        order_service_header_latitude, order_service_header_longitude,
        order_service_header_total_amount,
        order_service_header_create_at, order_service_header_create_by,
        order_service_header_update_at, order_service_header_update_by
    ) VALUES (
        @new_guid, @p_customer_guid,
        1, @p_address, @p_latitude, @p_longitude,
        @p_total_amount,
        SYSUTCDATETIME(), @p_customer_guid,
        SYSUTCDATETIME(), @p_customer_guid
    );

    INSERT INTO [order_service_body] (
        order_service_body_guid, order_service_body_header_guid,
        order_service_body_service_guid, order_service_body_description,
        order_service_body_status,
        order_service_body_create_at, order_service_body_create_by,
        order_service_body_update_at, order_service_body_update_by
    ) VALUES (
        NEWID(), @new_guid,
        @p_service_id, @p_description,
        1,
        SYSUTCDATETIME(), @p_customer_guid,
        SYSUTCDATETIME(), @p_customer_guid
    );

    INSERT INTO [order_service_assignment] (
        order_service_assignment_guid, order_service_assignment_header_guid,
        order_service_assignment_status, order_service_assignment_broadcast_at,
        order_service_assignment_create_at, order_service_assignment_create_by,
        order_service_assignment_update_at, order_service_assignment_update_by
    ) VALUES (
        NEWID(), @new_guid,
        1, SYSUTCDATETIME(),
        SYSUTCDATETIME(), @p_customer_guid,
        SYSUTCDATETIME(), @p_customer_guid
    );

    COMMIT TRAN;
    SELECT @new_guid AS id, 0 AS error_code, '' AS error_message;
END;
GO




IF OBJECT_ID('dbo.API_ORDER_GET', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.API_ORDER_GET AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.API_ORDER_GET
    @p_order_guid UNIQUEIDENTIFIER
AS
BEGIN
    SET NOCOUNT ON;

    SELECT
        h.order_service_header_guid          AS id,
        h.order_service_header_customer_guid  AS customer_id,
        h.order_service_header_status         AS status,
        h.order_service_header_address        AS address,
        h.order_service_header_latitude        AS latitude,
        h.order_service_header_longitude       AS longitude,
        h.order_service_header_total_amount    AS total_amount,
        h.order_service_header_create_at       AS created_at
    FROM [order_service_header] h
    WHERE h.order_service_header_guid = @p_order_guid;

    SELECT
        b.order_service_body_guid             AS id,
        b.order_service_body_header_guid       AS order_id,
        b.order_service_body_service_guid       AS service_id,
        b.order_service_body_description       AS description,
        b.order_service_body_status            AS status
    FROM [order_service_body] b
    WHERE b.order_service_body_header_guid = @p_order_guid;

    SELECT
        a.order_service_assignment_guid           AS id,
        a.order_service_assignment_header_guid    AS order_id,
        a.order_service_assignment_technician_guid AS technician_id,
        a.order_service_assignment_status         AS status,
        a.order_service_assignment_broadcast_at    AS broadcast_at,
        a.order_service_assignment_accept_at      AS accept_at,
        a.order_service_assignment_arrive_at      AS arrive_at
    FROM [order_service_assignment] a
    WHERE a.order_service_assignment_header_guid = @p_order_guid;
END;
GO




IF OBJECT_ID('dbo.API_ORDER_LIST_BY_CUSTOMER', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.API_ORDER_LIST_BY_CUSTOMER AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.API_ORDER_LIST_BY_CUSTOMER
    @p_customer_guid UNIQUEIDENTIFIER,
    @p_limit         INT = 50,
    @p_offset        INT = 0
AS
BEGIN
    SET NOCOUNT ON;

    SELECT
        h.order_service_header_guid          AS id,
        h.order_service_header_customer_guid  AS customer_id,
        h.order_service_header_status         AS status,
        h.order_service_header_total_amount    AS total_amount,
        h.order_service_header_create_at       AS created_at
    FROM [order_service_header] h
    WHERE h.order_service_header_customer_guid = @p_customer_guid
    ORDER BY h.order_service_header_create_at DESC
    OFFSET @p_offset ROWS
    FETCH NEXT @p_limit ROWS ONLY;
END;
GO




IF OBJECT_ID('dbo.API_ORDER_LIST_BY_TECHNICIAN', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.API_ORDER_LIST_BY_TECHNICIAN AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.API_ORDER_LIST_BY_TECHNICIAN
    @p_technician_guid UNIQUEIDENTIFIER,
    @p_limit            INT = 50,
    @p_offset           INT = 0
AS
BEGIN
    SET NOCOUNT ON;

    SELECT
        h.order_service_header_guid          AS id,
        h.order_service_header_customer_guid  AS customer_id,
        h.order_service_header_status         AS status,
        h.order_service_header_total_amount    AS total_amount,
        h.order_service_header_create_at       AS created_at
    FROM [order_service_header] h
    INNER JOIN [order_service_assignment] a
        ON a.order_service_assignment_header_guid = h.order_service_header_guid
    WHERE a.order_service_assignment_technician_guid = @p_technician_guid
    ORDER BY h.order_service_header_create_at DESC
    OFFSET @p_offset ROWS
    FETCH NEXT @p_limit ROWS ONLY;
END;
GO





IF OBJECT_ID('dbo.API_ORDER_ASSIGN_TECHNICIAN', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.API_ORDER_ASSIGN_TECHNICIAN AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.API_ORDER_ASSIGN_TECHNICIAN
    @p_order_guid      UNIQUEIDENTIFIER,
    @p_technician_guid UNIQUEIDENTIFIER
AS
BEGIN
    SET NOCOUNT ON;

    UPDATE [order_service_assignment]
    SET order_service_assignment_technician_guid = @p_technician_guid,
        order_service_assignment_accept_at       = SYSUTCDATETIME(),
        order_service_assignment_status          = 2
    WHERE order_service_assignment_header_guid = @p_order_guid
      AND order_service_assignment_status = 1;

    IF @@ROWCOUNT = 0
    BEGIN
        SELECT @p_order_guid AS id, 1 AS error_code, N'lost the race' AS error_message;
        RETURN;
    END;

    UPDATE [order_service_header]
    SET order_service_header_status    = 2,
        order_service_header_update_at = SYSUTCDATETIME()
    WHERE order_service_header_guid = @p_order_guid;

    SELECT @p_order_guid AS id, 0 AS error_code, '' AS error_message;
END;
GO




IF OBJECT_ID('dbo.API_ORDER_UPDATE_STATUS', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.API_ORDER_UPDATE_STATUS AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.API_ORDER_UPDATE_STATUS
    @p_order_guid UNIQUEIDENTIFIER,
    @p_status     INT
AS
BEGIN
    SET NOCOUNT ON;

    UPDATE [order_service_header]
    SET order_service_header_status = @p_status,
        order_service_header_update_at = SYSUTCDATETIME()
    WHERE order_service_header_guid = @p_order_guid;

    IF @@ROWCOUNT = 0
        SELECT @p_order_guid AS id, 1 AS error_code, N'order not found' AS error_message;
    ELSE
        SELECT @p_order_guid AS id, 0 AS error_code, '' AS error_message;
END;
GO
