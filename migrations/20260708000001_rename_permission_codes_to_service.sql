SET NOCOUNT ON;
BEGIN TRANSACTION;

DECLARE @view_guid   UNIQUEIDENTIFIER;
DECLARE @create_guid UNIQUEIDENTIFIER;
DECLARE @update_guid UNIQUEIDENTIFIER;
DECLARE @delete_guid UNIQUEIDENTIFIER;

INSERT INTO dbo.[user_permission] (user_permission_guid, user_permission_code, user_permission_name, user_permission_module, user_permission_status)
SELECT NEWID(), N'SERVICE_VIEW', N'Service View', N'service', 1
WHERE NOT EXISTS (SELECT 1 FROM dbo.[user_permission] WHERE user_permission_code = N'SERVICE_VIEW');

INSERT INTO dbo.[user_permission] (user_permission_guid, user_permission_code, user_permission_name, user_permission_module, user_permission_status)
SELECT NEWID(), N'SERVICE_CREATE', N'Service Create', N'service', 1
WHERE NOT EXISTS (SELECT 1 FROM dbo.[user_permission] WHERE user_permission_code = N'SERVICE_CREATE');

INSERT INTO dbo.[user_permission] (user_permission_guid, user_permission_code, user_permission_name, user_permission_module, user_permission_status)
SELECT NEWID(), N'SERVICE_UPDATE', N'Service Update', N'service', 1
WHERE NOT EXISTS (SELECT 1 FROM dbo.[user_permission] WHERE user_permission_code = N'SERVICE_UPDATE');

INSERT INTO dbo.[user_permission] (user_permission_guid, user_permission_code, user_permission_name, user_permission_module, user_permission_status)
SELECT NEWID(), N'SERVICE_DELETE', N'Service Delete', N'service', 1
WHERE NOT EXISTS (SELECT 1 FROM dbo.[user_permission] WHERE user_permission_code = N'SERVICE_DELETE');

SELECT @view_guid   = user_permission_guid FROM dbo.[user_permission] WHERE user_permission_code = N'SERVICE_VIEW';
SELECT @create_guid = user_permission_guid FROM dbo.[user_permission] WHERE user_permission_code = N'SERVICE_CREATE';
SELECT @update_guid = user_permission_guid FROM dbo.[user_permission] WHERE user_permission_code = N'SERVICE_UPDATE';
SELECT @delete_guid = user_permission_guid FROM dbo.[user_permission] WHERE user_permission_code = N'SERVICE_DELETE';

UPDATE urp SET urp.user_role_permission_permission_guid = @view_guid
FROM dbo.[user_role_permission] urp
JOIN dbo.[user_permission] p ON p.user_permission_guid = urp.user_role_permission_permission_guid
WHERE p.user_permission_code IN (N'CATEGORY_JOB_MAIN_VIEW', N'CATEGORY_JOB_SERVICE_MAIN_VIEW', N'CATEGORY_JOB_SERVICE_SUB_VIEW');

UPDATE ov SET ov.user_permission_override_permission_guid = @view_guid
FROM dbo.[user_permission_override] ov
JOIN dbo.[user_permission] p ON p.user_permission_guid = ov.user_permission_override_permission_guid
WHERE p.user_permission_code IN (N'CATEGORY_JOB_MAIN_VIEW', N'CATEGORY_JOB_SERVICE_MAIN_VIEW', N'CATEGORY_JOB_SERVICE_SUB_VIEW');

UPDATE urp SET urp.user_role_permission_permission_guid = @create_guid
FROM dbo.[user_role_permission] urp
JOIN dbo.[user_permission] p ON p.user_permission_guid = urp.user_role_permission_permission_guid
WHERE p.user_permission_code IN (N'CATEGORY_JOB_MAIN_CREATE', N'CATEGORY_JOB_SERVICE_MAIN_CREATE', N'CATEGORY_JOB_SERVICE_SUB_CREATE');

UPDATE ov SET ov.user_permission_override_permission_guid = @create_guid
FROM dbo.[user_permission_override] ov
JOIN dbo.[user_permission] p ON p.user_permission_guid = ov.user_permission_override_permission_guid
WHERE p.user_permission_code IN (N'CATEGORY_JOB_MAIN_CREATE', N'CATEGORY_JOB_SERVICE_MAIN_CREATE', N'CATEGORY_JOB_SERVICE_SUB_CREATE');

UPDATE urp SET urp.user_role_permission_permission_guid = @update_guid
FROM dbo.[user_role_permission] urp
JOIN dbo.[user_permission] p ON p.user_permission_guid = urp.user_role_permission_permission_guid
WHERE p.user_permission_code IN (N'CATEGORY_JOB_MAIN_UPDATE', N'CATEGORY_JOB_SERVICE_MAIN_UPDATE', N'CATEGORY_JOB_SERVICE_SUB_UPDATE');

UPDATE ov SET ov.user_permission_override_permission_guid = @update_guid
FROM dbo.[user_permission_override] ov
JOIN dbo.[user_permission] p ON p.user_permission_guid = ov.user_permission_override_permission_guid
WHERE p.user_permission_code IN (N'CATEGORY_JOB_MAIN_UPDATE', N'CATEGORY_JOB_SERVICE_MAIN_UPDATE', N'CATEGORY_JOB_SERVICE_SUB_UPDATE');

UPDATE urp SET urp.user_role_permission_permission_guid = @delete_guid
FROM dbo.[user_role_permission] urp
JOIN dbo.[user_permission] p ON p.user_permission_guid = urp.user_role_permission_permission_guid
WHERE p.user_permission_code IN (N'CATEGORY_JOB_MAIN_DELETE', N'CATEGORY_JOB_SERVICE_MAIN_DELETE', N'CATEGORY_JOB_SERVICE_SUB_DELETE');

UPDATE ov SET ov.user_permission_override_permission_guid = @delete_guid
FROM dbo.[user_permission_override] ov
JOIN dbo.[user_permission] p ON p.user_permission_guid = ov.user_permission_override_permission_guid
WHERE p.user_permission_code IN (N'CATEGORY_JOB_MAIN_DELETE', N'CATEGORY_JOB_SERVICE_MAIN_DELETE', N'CATEGORY_JOB_SERVICE_SUB_DELETE');

DELETE FROM dbo.[user_permission]
WHERE user_permission_code LIKE N'CATEGORY_JOB[_]%' ESCAPE '_';

COMMIT;
GO
