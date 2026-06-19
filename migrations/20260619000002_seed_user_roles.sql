-- ============================================================================
-- Seed default user_role catalog (M14-prep)
--
-- Inserts the 4 system roles used by Rust enum Role. The GUIDs are stable
-- so dev / e2e tests can reference them without looking them up.
--
-- Idempotent: each INSERT wrapped in IF NOT EXISTS check.
--
-- ponytail: roles are seeded as system-managed (user_role_is_master = 1 for
-- super_admin). Per-role permissions are a separate table (user_role_permission)
-- that lands in M15+ alongside the permission catalog.
-- ============================================================================

IF NOT EXISTS (SELECT 1 FROM [user_role] WHERE user_role_code = 'customer')
INSERT INTO [user_role] (
    user_role_guid, user_role_code, user_role_name, user_role_is_master,
    user_role_data_scope_type, user_role_status
)
VALUES (
    '11111111-1111-1111-1111-000000000001', 'customer', 'Customer', 0,
    'own', 1
);
GO

IF NOT EXISTS (SELECT 1 FROM [user_role] WHERE user_role_code = 'technician')
INSERT INTO [user_role] (
    user_role_guid, user_role_code, user_role_name, user_role_is_master,
    user_role_data_scope_type, user_role_status
)
VALUES (
    '11111111-1111-1111-1111-000000000002', 'technician', 'Technician', 0,
    'own', 1
);
GO

IF NOT EXISTS (SELECT 1 FROM [user_role] WHERE user_role_code = 'admin')
INSERT INTO [user_role] (
    user_role_guid, user_role_code, user_role_name, user_role_is_master,
    user_role_data_scope_type, user_role_status
)
VALUES (
    '11111111-1111-1111-1111-000000000003', 'admin', 'Admin', 0,
    'department', 1
);
GO

IF NOT EXISTS (SELECT 1 FROM [user_role] WHERE user_role_code = 'super_admin')
INSERT INTO [user_role] (
    user_role_guid, user_role_code, user_role_name, user_role_is_master,
    user_role_data_scope_type, user_role_status
)
VALUES (
    '11111111-1111-1111-1111-000000000004', 'super_admin', 'Super Admin', 1,
    'all', 1
);
GO
