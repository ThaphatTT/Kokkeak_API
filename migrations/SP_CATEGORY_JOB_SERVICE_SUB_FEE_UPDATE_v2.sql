ALTER PROCEDURE dbo.SP_CATEGORY_JOB_SERVICE_SUB_FEE_UPDATE
    @p_category_job_service_sub_fee_guid varchar(50),

    @p_category_job_service_sub_fee_header_la nvarchar(255) = NULL,
    @p_category_job_service_sub_fee_description_la nvarchar(max) = NULL,

    @p_category_job_service_sub_fee_header_en nvarchar(255) = NULL,
    @p_category_job_service_sub_fee_description_en nvarchar(max) = NULL,

    @p_category_job_service_sub_fee_header_th nvarchar(255) = NULL,
    @p_category_job_service_sub_fee_description_th nvarchar(max) = NULL,

    @p_category_job_service_sub_fee_header_zh nvarchar(255) = NULL,
    @p_category_job_service_sub_fee_description_zh nvarchar(max) = NULL,

    @p_category_job_service_sub_fee_price decimal(18, 2) = NULL,
    @p_category_job_service_sub_fee_status int = NULL,
    @p_category_job_service_sub_fee_icon nvarchar(255) = NULL,

    @p_update_by varchar(50) = NULL
AS
BEGIN
    SET NOCOUNT ON;
    SET XACT_ABORT ON;

    BEGIN TRY
        BEGIN TRANSACTION;

        SET @p_category_job_service_sub_fee_guid =
            NULLIF(LTRIM(RTRIM(@p_category_job_service_sub_fee_guid)), '');

        IF @p_category_job_service_sub_fee_guid IS NULL
        BEGIN
            ROLLBACK TRANSACTION;

            SELECT
                CAST(0 AS bit) AS success,
                'GUID_REQUIRED' AS code,
                N'กรุณาระบุ category_job_service_sub_fee_guid' AS message,
                @p_category_job_service_sub_fee_guid AS category_job_service_sub_fee_guid;
            RETURN;
        END;

        IF NOT EXISTS (
            SELECT 1
            FROM dbo.category_job_service_sub_fee
            WHERE category_job_service_sub_fee_guid = @p_category_job_service_sub_fee_guid
        )
        BEGIN
            ROLLBACK TRANSACTION;

            SELECT
                CAST(0 AS bit) AS success,
                'NOT_FOUND' AS code,
                N'ไม่พบข้อมูลค่าใช้จ่ายแฝงที่ต้องการแก้ไข' AS message,
                @p_category_job_service_sub_fee_guid AS category_job_service_sub_fee_guid;
            RETURN;
        END;

        IF @p_category_job_service_sub_fee_status IS NOT NULL
           AND @p_category_job_service_sub_fee_status NOT IN (0, 1)
        BEGIN
            ROLLBACK TRANSACTION;

            SELECT
                CAST(0 AS bit) AS success,
                'INVALID_STATUS' AS code,
                N'status ต้องเป็น 0 (ปิดใช้งาน) หรือ 1 (เปิดใช้งาน)' AS message,
                @p_category_job_service_sub_fee_guid AS category_job_service_sub_fee_guid;
            RETURN;
        END;

        IF @p_category_job_service_sub_fee_price IS NOT NULL
           AND @p_category_job_service_sub_fee_price < 0
        BEGIN
            ROLLBACK TRANSACTION;

            SELECT
                CAST(0 AS bit) AS success,
                'INVALID_PRICE' AS code,
                N'ราคาต้องไม่น้อยกว่า 0' AS message,
                @p_category_job_service_sub_fee_guid AS category_job_service_sub_fee_guid;
            RETURN;
        END;

        IF @p_category_job_service_sub_fee_price IS NOT NULL
           AND @p_category_job_service_sub_fee_price > 9999999999999999.99
        BEGIN
            ROLLBACK TRANSACTION;

            SELECT
                CAST(0 AS bit) AS success,
                'PRICE_OUT_OF_RANGE' AS code,
                N'ราคาเกิน decimal(18,2) range' AS message,
                @p_category_job_service_sub_fee_guid AS category_job_service_sub_fee_guid;
            RETURN;
        END;

        IF (
            (@p_category_job_service_sub_fee_header_la IS NOT NULL AND LEN(@p_category_job_service_sub_fee_header_la) > 255)
            OR (@p_category_job_service_sub_fee_header_en IS NOT NULL AND LEN(@p_category_job_service_sub_fee_header_en) > 255)
            OR (@p_category_job_service_sub_fee_header_th IS NOT NULL AND LEN(@p_category_job_service_sub_fee_header_th) > 255)
            OR (@p_category_job_service_sub_fee_header_zh IS NOT NULL AND LEN(@p_category_job_service_sub_fee_header_zh) > 255)
            OR (@p_category_job_service_sub_fee_icon IS NOT NULL AND LEN(@p_category_job_service_sub_fee_icon) > 255)
        )
        BEGIN
            ROLLBACK TRANSACTION;

            SELECT
                CAST(0 AS bit) AS success,
                'HEADER_TOO_LONG' AS code,
                N'header หรือ icon ต้องไม่เกิน 255 ตัวอักษร' AS message,
                @p_category_job_service_sub_fee_guid AS category_job_service_sub_fee_guid;
            RETURN;
        END;

        UPDATE dbo.category_job_service_sub_fee
        SET
            category_job_service_sub_fee_header_la =
                ISNULL(@p_category_job_service_sub_fee_header_la, category_job_service_sub_fee_header_la),
            category_job_service_sub_fee_description_la =
                ISNULL(@p_category_job_service_sub_fee_description_la, category_job_service_sub_fee_description_la),

            category_job_service_sub_fee_header_en =
                ISNULL(@p_category_job_service_sub_fee_header_en, category_job_service_sub_fee_header_en),
            category_job_service_sub_fee_description_en =
                ISNULL(@p_category_job_service_sub_fee_description_en, category_job_service_sub_fee_description_en),

            category_job_service_sub_fee_header_th =
                ISNULL(@p_category_job_service_sub_fee_header_th, category_job_service_sub_fee_header_th),
            category_job_service_sub_fee_description_th =
                ISNULL(@p_category_job_service_sub_fee_description_th, category_job_service_sub_fee_description_th),

            category_job_service_sub_fee_header_zh =
                ISNULL(@p_category_job_service_sub_fee_header_zh, category_job_service_sub_fee_header_zh),
            category_job_service_sub_fee_description_zh =
                ISNULL(@p_category_job_service_sub_fee_description_zh, category_job_service_sub_fee_description_zh),

            category_job_service_sub_fee_price =
                ISNULL(@p_category_job_service_sub_fee_price, category_job_service_sub_fee_price),
            category_job_service_sub_fee_status =
                ISNULL(@p_category_job_service_sub_fee_status, category_job_service_sub_fee_status),
            category_job_service_sub_fee_icon =
                ISNULL(@p_category_job_service_sub_fee_icon, category_job_service_sub_fee_icon),

            category_job_service_sub_fee_update_at = SYSDATETIME(),
            category_job_service_sub_fee_update_by = ISNULL(NULLIF(LTRIM(RTRIM(@p_update_by)), ''), category_job_service_sub_fee_update_by)
        WHERE category_job_service_sub_fee_guid = @p_category_job_service_sub_fee_guid;

        COMMIT TRANSACTION;

        SELECT
            CAST(1 AS bit) AS success,
            'UPDATE_SUCCESS' AS code,
            N'แก้ไขข้อมูลค่าใช้จ่ายแฝงสำเร็จ' AS message,
            @p_category_job_service_sub_fee_guid AS category_job_service_sub_fee_guid;
    END TRY
    BEGIN CATCH
        IF @@TRANCOUNT > 0
            ROLLBACK TRANSACTION;

        SELECT
            CAST(0 AS bit) AS success,
            'UPDATE_ERROR' AS code,
            ERROR_MESSAGE() AS message,
            @p_category_job_service_sub_fee_guid AS category_job_service_sub_fee_guid;
    END CATCH
END;
