










IF OBJECT_ID('dbo.API_CHAT_CREATE_ROOM', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.API_CHAT_CREATE_ROOM AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.API_CHAT_CREATE_ROOM
    @p_participant_a  UNIQUEIDENTIFIER,
    @p_participant_b  UNIQUEIDENTIFIER,
    @p_is_group       BIT = 0
AS
BEGIN
    SET NOCOUNT ON;
    DECLARE @room UNIQUEIDENTIFIER;

    IF @p_is_group = 0
    BEGIN
        SELECT @room = room_id
        FROM [chat_rooms]
        WHERE room_is_group = 0
          AND room_status = 1
          AND (
              (participant_a = @p_participant_a AND participant_b = @p_participant_b)
              OR
              (participant_a = @p_participant_b AND participant_b = @p_participant_a)
          );
        IF @room IS NOT NULL
        BEGIN
            SELECT @room AS id, 0 AS error_code, '' AS error_message;
            RETURN;
        END;
    END;

    SET @room = NEWID();
    INSERT INTO [chat_rooms] (
        room_id, room_is_group, room_status,
        participant_a, participant_b,
        room_created_at
    ) VALUES (
        @room, @p_is_group, 1,
        @p_participant_a, @p_participant_b,
        SYSUTCDATETIME()
    );
    SELECT @room AS id, 0 AS error_code, '' AS error_message;
END;
GO




IF OBJECT_ID('dbo.API_CHAT_LIST_ROOMS', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.API_CHAT_LIST_ROOMS AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.API_CHAT_LIST_ROOMS
    @p_user_guid UNIQUEIDENTIFIER
AS
BEGIN
    SET NOCOUNT ON;

    SELECT
        r.room_id           AS id,
        r.room_is_group     AS is_group,
        r.room_status       AS status,
        r.participant_a     AS participant_a,
        r.participant_b     AS participant_b,
        r.room_created_at   AS created_at,
        r.room_last_msg_at  AS last_msg_at,
        (SELECT COUNT(*)
         FROM [chat_messages] m
         WHERE m.room_id = r.room_id
           AND m.msg_read_by NOT LIKE '%' + CAST(@p_user_guid AS NVARCHAR(36)) + '%'
        ) AS unread_count
    FROM [chat_rooms] r
    WHERE r.room_status = 1
      AND (r.participant_a = @p_user_guid OR r.participant_b = @p_user_guid)
    ORDER BY ISNULL(r.room_last_msg_at, r.room_created_at) DESC;
END;
GO




IF OBJECT_ID('dbo.API_CHAT_SEND_MESSAGE', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.API_CHAT_SEND_MESSAGE AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.API_CHAT_SEND_MESSAGE
    @p_room_guid      UNIQUEIDENTIFIER,
    @p_sender_guid    UNIQUEIDENTIFIER,
    @p_body           NVARCHAR(MAX)
AS
BEGIN
    SET NOCOUNT ON;
    DECLARE @msg UNIQUEIDENTIFIER = NEWID();

    INSERT INTO [chat_messages] (
        msg_id, room_id, sender_guid, body,
        sent_at, msg_status, msg_read_by
    ) VALUES (
        @msg, @p_room_guid, @p_sender_guid, @p_body,
        SYSUTCDATETIME(), 1, ''
    );

    UPDATE [chat_rooms]
    SET room_last_msg_at = SYSUTCDATETIME()
    WHERE room_id = @p_room_guid;

    SELECT @msg AS id, 0 AS error_code, '' AS error_message;
END;
GO




IF OBJECT_ID('dbo.API_CHAT_LIST_MESSAGES', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.API_CHAT_LIST_MESSAGES AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.API_CHAT_LIST_MESSAGES
    @p_room_guid UNIQUEIDENTIFIER,
    @p_limit     INT = 50,
    @p_before    DATETIME2(7) = NULL
AS
BEGIN
    SET NOCOUNT ON;

    SELECT
        m.msg_id      AS id,
        m.room_id     AS room_id,
        m.sender_guid AS sender_id,
        m.body        AS body,
        m.sent_at     AS sent_at,
        m.msg_status  AS status,
        m.msg_read_by AS read_by
    FROM [chat_messages] m
    WHERE m.room_id = @p_room_guid
      AND (@p_before IS NULL OR m.sent_at < @p_before)
    ORDER BY m.sent_at DESC
    OFFSET 0 ROWS
    FETCH NEXT @p_limit ROWS ONLY;
END;
GO




IF OBJECT_ID('dbo.API_CHAT_MARK_READ', 'P') IS NULL
EXEC ('CREATE PROCEDURE dbo.API_CHAT_MARK_READ AS BEGIN SET NOCOUNT ON; END');
GO

ALTER PROCEDURE dbo.API_CHAT_MARK_READ
    @p_room_guid UNIQUEIDENTIFIER,
    @p_user_guid UNIQUEIDENTIFIER
AS
BEGIN
    SET NOCOUNT ON;

    UPDATE [chat_messages]
    SET msg_read_by = CASE
        WHEN msg_read_by LIKE '%' + CAST(@p_user_guid AS NVARCHAR(36)) + '%'
            THEN msg_read_by
        WHEN LEN(msg_read_by) = 0
            THEN CAST(@p_user_guid AS NVARCHAR(36))
        ELSE msg_read_by + ',' + CAST(@p_user_guid AS NVARCHAR(36))
    END
    WHERE room_id = @p_room_guid
      AND msg_read_by NOT LIKE '%' + CAST(@p_user_guid AS NVARCHAR(36)) + '%';

    SELECT @p_room_guid AS id, 0 AS error_code, '' AS error_message;
END;
GO
