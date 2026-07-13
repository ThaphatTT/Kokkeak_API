use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::Modify;
use utoipa::OpenApi;

use crate::handlers;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Kokkeak API",
        version = "0.1.0",
        description = "Handyman / technician marketplace backend (Laos). \
            Mobile-first JSON over HTTPS. All responses use the standard \
            envelope: `{ success, data, error, meta }`. Errors include a \
            machine-readable `error.code` for programmatic handling. \
            Protected POSTs require `Idempotency-Key`. \
            See GET /api/error-codes.json for the full catalog.",
        contact(name = "Kokkeak Team"),
    ),
    paths(

        handlers::health::healthz,
        handlers::health::readyz,

        handlers::auth::register,
        handlers::auth::login,
        handlers::auth::refresh,
        handlers::auth::logout,
        handlers::auth::list_sessions,
        handlers::auth::revoke_session,

        handlers::user::get_me,
        handlers::catalog::list_services,

        handlers::category_job_main::list_category_job_mains,
                        handlers::category_job_main::autocomplete_category_job_mains,
                        handlers::category_job_main::get_category_job_main,
                        handlers::category_job_main::create_category_job_main_admin,
                        handlers::category_job_main::update_category_job_main_admin,
                        handlers::category_job_main::delete_category_job_main_admin,

                handlers::category_job_service_main::list_category_job_service_mains,
                                handlers::category_job_service_main::autocomplete_category_job_service_mains,
                                handlers::category_job_service_main::get_category_job_service_main,
                                handlers::category_job_service_main::create_category_job_service_main_admin,
                                handlers::category_job_service_main::update_category_job_service_main_admin,
                                handlers::category_job_service_main::delete_category_job_service_main_admin,

                handlers::category_job_service_sub::list_category_job_service_subs,
                handlers::category_job_service_sub::get_category_job_service_sub,
                handlers::category_job_service_sub::list_category_job_service_sub_images,
                handlers::category_job_service_sub::create_category_job_service_sub_admin,
                handlers::category_job_service_sub::update_category_job_service_sub_admin,
                handlers::category_job_service_sub::delete_category_job_service_sub_admin,

                                handlers::master::list_countries,
                        handlers::master::autocomplete_user_department_team,
                        handlers::master::autocomplete_user_department,
                        handlers::master::autocomplete_master_positions,

        handlers::order::list_my_orders,
        handlers::order::list_assigned_orders,
        handlers::order::create_order,

        handlers::payment::list_my_payments,
        handlers::payment::get_payment,
        handlers::payment::create_payment,
        handlers::payment::confirm_payment,

                        handlers::payment::list_payouts_admin,
                        handlers::payment::mark_payout_paid_admin,
                        handlers::admin::create_user_admin,
                                                handlers::admin::admin_insert_user_full,
                                                handlers::admin::admin_update_user_full,
                                                handlers::admin::list_users_admin,
                                                handlers::admin::list_user_permissions_admin,
                                                handlers::admin::get_user_detail_full_admin,
                                handlers::admin::list_permissions,
                                handlers::admin::update_permissions_admin,

                handlers::permission::update_permission_overrides,
    ),
    components(
        schemas(

                        handlers::auth::RegisterRequest,
                        handlers::auth::LoginRequest,
                        handlers::auth::RefreshRequest,
                        handlers::auth::AuthResponse,
                        handlers::auth::LogoutResponse,
                        kokkak_domain::SessionInfo,
                        handlers::catalog::ListQuery,
                        handlers::catalog::ServiceItem,
                        handlers::category_job_main::ListCategoryJobMainQuery,
                                                handlers::category_job_main::ListCategoryJobMainMeta,
                                                handlers::category_job_main::ListCategoryJobMainResponse,
                                                handlers::category_job_main::AutocompleteCategoryJobMainQuery,
                                                handlers::category_job_main::CreateCategoryJobMainRequest,
                                                handlers::category_job_main::UpdateCategoryJobMainRequest,
                                                kokkak_domain::CategoryJobMainRow,
                                                kokkak_domain::CategoryJobMainListInput,
                                                kokkak_domain::CategoryJobMainPage,
                                                kokkak_domain::CategoryJobMainCreateInput,
                                                kokkak_domain::CategoryJobMainUpdateInput,
                                                kokkak_domain::CategoryJobMainCreateResult,
                                                kokkak_domain::CategoryJobMainUpdateResult,
                                                kokkak_domain::CategoryJobMainDeleteResult,
                                                kokkak_domain::CategoryJobMainAutocompleteInput,
                                                kokkak_domain::CategoryJobMainAutocompleteRow,
                                                kokkak_domain::CategoryJobMainDetailRow,
                                                handlers::category_job_service_main::ListCategoryJobServiceMainQuery,
                                                                                                                        handlers::category_job_service_main::ListCategoryJobServiceMainResponse,
                                                                                                                        handlers::category_job_service_main::AutocompleteCategoryJobServiceMainQuery,
                                                                                                                        handlers::category_job_service_main::CreateCategoryJobServiceMainRequest,
                                                                                                                        handlers::category_job_service_main::UpdateCategoryJobServiceMainRequest,
                                                                                                                        kokkak_domain::CategoryJobServiceMainRow,
                                                                                                                        kokkak_domain::CategoryJobServiceMainListInput,
                                                                                                                        kokkak_domain::CategoryJobServiceMainCreateInput,
                                                                                                                        kokkak_domain::CategoryJobServiceMainUpdateInput,
                                                                                                                        kokkak_domain::CategoryJobServiceMainCreateResult,
                                                                                                                        kokkak_domain::CategoryJobServiceMainUpdateResult,
                                                                                                                        kokkak_domain::CategoryJobServiceMainDeleteResult,
                                                                                                                        kokkak_domain::CategoryJobServiceMainAutocompleteInput,
                                                                                                                        kokkak_domain::CategoryJobServiceMainAutocompleteRow,
                                                                                                                        kokkak_domain::CategoryJobServiceMainDetailRow,
                                                handlers::category_job_service_sub::ListCategoryJobServiceSubQuery,
                                                handlers::category_job_service_sub::ListCategoryJobServiceSubResponse,
                                                handlers::category_job_service_sub::CreateCategoryJobServiceSubRequest,
                                                handlers::category_job_service_sub::UpdateCategoryJobServiceSubRequest,
                                                kokkak_domain::CategoryJobServiceSubRow,
                                                kokkak_domain::CategoryJobServiceSubImageRow,
                                                kokkak_domain::CategoryJobServiceSubFeeRow,
                                                kokkak_domain::CategoryJobServiceSubWarrantyRow,
                                                kokkak_domain::CategoryJobServiceSubDetailBundle,
                                                kokkak_domain::CategoryJobServiceSubDetailRow,
                                                kokkak_domain::CategoryJobServiceSubDetailWarrantyRow,
                                                kokkak_domain::CategoryJobServiceSubDetailFeeRow,
                                                kokkak_domain::CategoryJobServiceSubDetailImageRow,
                                                kokkak_domain::CategoryJobServiceSubImageInput,
                                                kokkak_domain::CategoryJobServiceSubCreateInput,
                                                kokkak_domain::CategoryJobServiceSubUpdateInput,
                                                kokkak_domain::CategoryJobServiceSubCreateResult,
                                                kokkak_domain::CategoryJobServiceSubUpdateResult,
                                                kokkak_domain::CategoryJobServiceSubDeleteResult,
                                                kokkak_domain::CategoryJobServiceSubImageCreateResult,
                                                kokkak_domain::CategoryJobServiceSubImageDeleteResult,
                                                handlers::category_job_service_sub::CreateCategoryJobServiceSubSpRequest,
                                                kokkak_domain::CategoryJobServiceSubCreateSpImageInput,
                                                kokkak_domain::CategoryJobServiceSubCreateSpWarrantyInput,
                                                kokkak_domain::CategoryJobServiceSubCreateSpFeeInput,
                                                kokkak_domain::CategoryJobServiceSubCreateSpInput,
                                                kokkak_domain::CategoryJobServiceSubCreateSpResult,
                                                handlers::category_job_service_sub_fee::ListCategoryJobServiceSubFeeQuery,
                                                                                                handlers::category_job_service_sub_fee::ListCategoryJobServiceSubFeeResponse,
                                                                                                handlers::category_job_service_sub_fee::CreateCategoryJobServiceSubFeeRequest,
                                                                                                handlers::category_job_service_sub_fee::UpdateCategoryJobServiceSubFeeRequest,
                                                                                                kokkak_domain::CategoryJobServiceSubFeeAdminRow,
                                                                                                kokkak_domain::CategoryJobServiceSubFeeCreateResult,
                                                                                                kokkak_domain::CategoryJobServiceSubFeeUpdateResult,
                                                handlers::category_job_service_sub_warranty::ListCategoryJobServiceSubWarrantyQuery,
                                                handlers::category_job_service_sub_warranty::ListCategoryJobServiceSubWarrantyResponse,
                                                handlers::category_job_service_sub_warranty::CreateCategoryJobServiceSubWarrantyRequest,
                                                handlers::category_job_service_sub_warranty::UpdateCategoryJobServiceSubWarrantyRequest,
                                                kokkak_domain::CategoryJobServiceSubWarrantyDetailRow,
                                                kokkak_domain::CategoryJobServiceSubWarrantyCreateResult,
                                                kokkak_domain::CategoryJobServiceSubWarrantyUpdateResult,
                                                kokkak_domain::CategoryJobServiceSubWarrantyDeleteResult,
                        handlers::master::CountriesQuery,
                                                kokkak_domain::MasterDropdownRow,
                                                                                                handlers::master::AutocompleteUserDepartmentTeamQuery,
                                                                                                kokkak_domain::UserDepartmentTeamAutocompleteRow,
                                                                                                handlers::master::AutocompleteUserDepartmentQuery,
                                                handlers::master::PositionsAutocompleteQuery,
                                                kokkak_domain::MasterPositionAutocompleteRow,
                        handlers::admin::CreateUserRequest,

                                                handlers::admin::AdminInsertUserRequest,
                                                handlers::admin::AdminInsertUserResponse,

                                                handlers::admin::AdminUpdateUserRequest,
                                                handlers::admin::AdminUpdateUserResponse,
                                                handlers::admin::WeeklyScheduleDto,
                                                handlers::admin::DayScheduleDto,

                        kokkak_domain::AdminUserDetail,
                        kokkak_domain::AdminUserDetailUsername,
                        kokkak_domain::AdminUserDetailProfileImage,
                        kokkak_domain::AdminUserDetailCountry,
                        kokkak_domain::AdminUserDetailCompany,
                        kokkak_domain::AdminUserDetailRoles,
                        kokkak_domain::AdminUserDetailScope,
                        kokkak_domain::AdminUserDetailPosition,
                        kokkak_domain::AdminUserDetailSalary,
                        kokkak_domain::AdminUserDetailBankAccount,
                        kokkak_domain::AdminUserDetailAttachment,
                                    handlers::admin::ListUsersQuery,
                                                handlers::admin::ListUsersResponse,
                                                handlers::admin::PermissionsQuery,
                        handlers::admin::UpdatePermissionsRequest,
                        handlers::admin::UpdatePermissionsResponse,
                        handlers::admin::PermissionUpdateItem,
                        handlers::admin::PermissionUpdateResultItem,
                        kokkak_domain::PermissionUpdateRow,

                        kokkak_domain::PublicUser,
                                    kokkak_domain::UserListRow,
                                    kokkak_domain::admin_user::UserListPagingRow,
                                    kokkak_domain::admin_user::AdminUserListPagingPage,
                        kokkak_domain::ServiceCategory,
            kokkak_domain::Order,
            kokkak_domain::OrderStatus,
            kokkak_domain::Payment,
            kokkak_domain::PaymentStatus,
            kokkak_domain::Payout,
            kokkak_domain::PayoutStatus,
            kokkak_domain::Role,
            kokkak_domain::UserRolePermission,
            kokkak_domain::UserRolePermissionRow,
            kokkak_domain::UserRoleWithPermissions,

            kokkak_domain::PermissionUserListRow,
            kokkak_domain::PermissionUserDetailRow,
            kokkak_domain::PermissionUserGroupEntry,
            kokkak_domain::PermissionUserGroup,

            kokkak_domain::PermissionOverrideUpdateItem,
            kokkak_domain::PermissionOverrideUpdateResult,
            handlers::permission::UpdatePermissionOverridesRequest,
            handlers::permission::UpdatePermissionOverridesResponse,

            ApiError,
            ApiErrorBody,
        ),
    ),
    modifiers(&SecurityAddon),
    tags(
        (name = "health", description = "Liveness + readiness probes (no auth)"),
        (name = "auth", description = "Login, register, refresh, logout"),
        (name = "users", description = "Current user profile"),
        (name = "catalog", description = "Service category catalog (master data)"),
        (name = "orders", description = "Order lifecycle — requires Idempotency-Key on POST"),
        (name = "payments", description = "Payment intents — requires Idempotency-Key on POST"),
        (name = "category-job-main", description = "Top-level service category (web/mobile landing page) — read endpoints"),
                (name = "category-job-service-main", description = "Service items under each main category (web/mobile landing page) — read endpoints"),
                (name = "category-job-service-sub", description = "Sub-service items (ล้างแอร์ 9,000-12,000 BTU, ซ่อมท่อน้ำรั่ว, etc.) — read endpoints"),
                (name = "category-job-service-sub-fee", description = "Admin: localized sub-service fee catalogue (บริการเสริม เช่น ค่าขนส่ง, ค่าติดตั้ง) — paginated, keyword + locale filters"),
                (name = "admin", description = "Admin-only endpoints (requires admin JWT)"),
    )
)]
pub struct ApiDoc;

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_default();
        components.security_schemes.insert(
            "bearer_auth".into(),
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .build(),
            ),
        );
    }
}

#[derive(Clone, Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ErrorCodeEntry {
    pub code: &'static str,

    pub status: u16,

    pub description: &'static str,
}

pub fn error_codes_catalog() -> Vec<ErrorCodeEntry> {
    use kokkak_common::error_codes::ErrorCode;
    vec![
        (
            ErrorCode::BAD_REQUEST,
            400,
            "Request is malformed (invalid JSON, missing required field).",
        ),
        (
            ErrorCode::IDEMPOTENCY_KEY_REQUIRED,
            400,
            "`Idempotency-Key` header is missing or whitespace on a protected endpoint.",
        ),
        (
            ErrorCode::UNAUTHORIZED,
            401,
            "Credentials missing, wrong, or otherwise invalid.",
        ),
        (
            ErrorCode::INVALID_TOKEN,
            401,
            "Bearer token signature / format invalid.",
        ),
        (
            ErrorCode::TOKEN_EXPIRED,
            401,
            "Bearer token expired (`exp` claim in the past).",
        ),
        (
            ErrorCode::REFRESH_INVALID,
            401,
            "Refresh token rejected (revoked, malformed, or expired).",
        ),
        (
            ErrorCode::FORBIDDEN,
            403,
            "Authenticated but the role is not allowed on this endpoint.",
        ),
        (
            ErrorCode::ADMIN_REQUIRED,
            403,
            "Admin role required (admin-only endpoints).",
        ),
        (
            ErrorCode::NOT_A_PARTICIPANT,
            403,
            "Caller is not a participant of the target chat room.",
        ),
        (ErrorCode::NOT_FOUND, 404, "Resource not found."),
        (ErrorCode::ROOM_NOT_FOUND, 404, "Chat room not found."),
        (
            ErrorCode::CONFLICT,
            409,
            "State conflict (generic; prefer a more specific code).",
        ),
        (
            ErrorCode::USERNAME_TAKEN,
            409,
            "Username already taken (registration, admin user create).",
        ),
        (
            ErrorCode::PAYMENT_ALREADY_CAPTURED,
            409,
            "Payment already captured (cannot confirm twice).",
        ),
        (ErrorCode::VALIDATION, 422, "Semantic validation failure."),
        (
            ErrorCode::ROLE_NOT_ALLOWED,
            422,
            "Role string is not in the public-registration allow-list.",
        ),
        (
            ErrorCode::INVALID_BODY,
            422,
            "Chat message body was empty or too long.",
        ),
        (ErrorCode::RATE_LIMITED, 429, "Per-IP rate limit hit."),
        (
            ErrorCode::INTERNAL,
            500,
            "Unexpected internal error (catch-all).",
        ),
    ]
    .into_iter()
    .map(|(code, status, description)| ErrorCodeEntry {
        code,
        status,
        description,
    })
    .collect()
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ApiError {
    pub success: bool,

    pub data: Option<serde_json::Value>,

    pub error: ApiErrorBody,

    pub meta: Option<serde_json::Value>,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ApiErrorBody {
    pub code: String,

    pub message: String,
}
