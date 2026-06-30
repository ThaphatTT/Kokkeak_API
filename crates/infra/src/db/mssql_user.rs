//! SQL Server-backed `UserRepository` (M14.5 — stored procedures only).
//!
//! Implements [`UserRepository`] via tiberius + the NEW_DB v2 stored
//! procedures. No inline SQL — every operation is `EXEC dbo.API_USER_*`.
//!
//! ponytail: the executor is intentionally thin (one helper per repo).
//! Ceiling: SPs could be replaced by an ORM (diesel / sea-orm) when
//! the schema stabilizes; for now SPs give the DBA explicit control
//! over the multi-table JOINs + role lookup logic.
//!
//! ## Storage procedure contract
//!
//! Every `API_USER_*` SP follows the uniform output shape documented in
//! `migrations/20260620000001_sp_user.sql`. The Rust side reads the
//! first row of the first result set and maps `error_code` to
//! `RepoError`:
//! - `error_code = 0` → ok
//! - `error_code = 1` → `NotFound`
//! - `error_code = 2` → `Conflict` (username taken)
//! - `error_code = 3` → `Backend` (validation / unknown)

use async_trait::async_trait;
use tiberius::ToSql;

use kokkak_domain::admin_user::{
    AdminInsertUserError, AdminInsertUserRequest, AdminInsertUserResult, DaySchedule,
};
use kokkak_domain::{Permission, RepoError, Role, User, UserListRow, UserRepository, UserStatus};
use uuid::Uuid;

use crate::db::mssql::{exec_sp, read_guid_str, read_i32, read_str, MssqlPool, SpError};

/// SQL Server-backed `UserRepository` (M14.5 — stored procedures).
#[derive(Clone)]
pub struct MssqlUserRepository {
    pool: MssqlPool,
}

impl MssqlUserRepository {
    /// Construct the repository with a shared `MssqlPool`.
    pub fn new(pool: MssqlPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserRepository for MssqlUserRepository {
    async fn find_by_id(&self, id: Uuid) -> Result<Option<User>, RepoError> {
        // GUID stored as varchar(50) in DB per project convention — bind
        // as String. SP_-prefixed SPs (e.g. SP_USER_FIND_BY_ID) accept
        // varchar(50); API_-prefixed SPs keep UNIQUEIDENTIFIER.
        let id_str = id.to_string();
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_USER_FIND_BY_ID @p_user_guid = @P1",
            &[&id_str as &dyn ToSql],
        )
        .await?;
        // First row: profile. Second row: roles + permissions CSV.
        let profile = rows
            .first()
            .ok_or_else(|| RepoError::Backend("SP_USER_FIND_BY_ID returned no row".into()))?;
        let user = row_to_user(profile)?;
        let (roles, permissions) = read_roles_and_permissions(&rows, 1)?;
        Ok(Some(User {
            roles,
            permissions,
            ..user
        }))
    }

    async fn find_by_username(&self, username: &str) -> Result<Option<User>, RepoError> {
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_USER_FIND_BY_USERNAME @p_username = @P1",
            &[&username as &dyn ToSql],
        )
        .await?;
        // Empty result → user not found.
        let profile = match rows.first() {
            None => return Ok(None),
            Some(r) => r,
        };
        let user = row_to_user(profile)?;
        // Roles CSV from the second result set.
        // Status (user_user_role.status=1, user_role.status=1) and
        // expire_at are filtered inside the SP (see
        // migrations/20260620000001_sp_user.sql + RDBMS Permssion.md
        // §1.8 + §3 Step 5) — we only see roles that already passed.
        // Effective permissions (role + allow − deny) + data scope
        // land in M15+ via a dedicated SP_USER_GET_EFFECTIVE_PERMISSIONS.
        let (roles, permissions) = read_roles_and_permissions(&rows, 1)?;
        Ok(Some(User {
            roles,
            permissions,
            ..user
        }))
    }

    async fn insert(&self, user: &User) -> Result<(), RepoError> {
        // API_USER_REGISTER takes the first role code only (multi-role
        // is rare; admin endpoint M15+ will use API_USER_SET_ROLES for
        // post-registration role changes). For now, register with the
        // first role and use API_USER_SET_ROLES for the rest.
        let role_code = user
            .roles
            .first()
            .map(|r| r.as_str())
            .ok_or_else(|| RepoError::Backend("at least one role required".into()))?;

        // 1. Register (creates user + username + first role).
        let reg_rows = exec_sp(
            &self.pool,
            "EXEC dbo.API_USER_REGISTER \
                @p_first_name = @P1, @p_last_name = @P2, \
                @p_username = @P3, @p_password_hash = @P4, \
                @p_role_code = @P5",
            &[
                &user.first_name as &dyn ToSql,
                &user.last_name as &dyn ToSql,
                &user.username as &dyn ToSql,
                &user.password_hash as &dyn ToSql,
                &role_code as &dyn ToSql,
            ],
        )
        .await?;
        let reg_row = reg_rows
            .first()
            .ok_or_else(|| RepoError::Backend("API_USER_REGISTER returned no row".into()))?;
        let err = read_i32(reg_row, "error_code").unwrap_or(3);
        let msg = read_str(reg_row, "error_message").unwrap_or_default();
        match SpError::from_code(err, msg) {
            SpError::None => Ok(()),
            SpError::Conflict => Err(RepoError::Conflict(msg.to_string())),
            SpError::NotFound => Err(RepoError::Backend(format!("USER_REGISTER: {}", msg))),
            SpError::BadInput => Err(RepoError::Backend(format!("validation: {msg}"))),
            SpError::Other => Err(RepoError::Backend(msg.to_string())),
        }?;

        // 2. If the user has more than one role, append via SET_ROLES.
        if user.roles.len() > 1 {
            let extra: Vec<&str> = user.roles[1..].iter().map(|r| r.as_str()).collect();
            let csv = extra.join(",");
            let set_rows = exec_sp(
                &self.pool,
                "EXEC dbo.API_USER_SET_ROLES \
                    @p_user_guid = @P1, @p_role_codes = @P2",
                &[&user.id as &dyn ToSql, &csv as &dyn ToSql],
            )
            .await?;
            let _ = set_rows; // API_USER_SET_ROLES always returns ok in practice
        }

        Ok(())
    }

    async fn update(&self, user: &User) -> Result<(), RepoError> {
        let status_i32 = user.status.as_i32();
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.API_USER_UPDATE \
                @p_user_guid = @P1, @p_first_name = @P2, @p_last_name = @P3, \
                @p_password_hash = @P4, @p_status = @P5",
            &[
                &user.id as &dyn ToSql,
                &user.first_name as &dyn ToSql,
                &user.last_name as &dyn ToSql,
                &user.password_hash as &dyn ToSql,
                &status_i32 as &dyn ToSql,
            ],
        )
        .await?;
        let row = rows
            .first()
            .ok_or_else(|| RepoError::Backend("API_USER_UPDATE returned no row".into()))?;
        let err = read_i32(row, "error_code").unwrap_or(0);
        let msg = read_str(row, "error_message").unwrap_or_default();
        match SpError::from_code(err, msg) {
            SpError::None => Ok(()),
            SpError::NotFound => Err(RepoError::NotFound(msg.to_string())),
            _ => Err(RepoError::Backend(msg.to_string())),
        }
    }

    // --------------------------------------------------------------------
    // M16: admin user-list SP (permission-detail moved to
    // `MssqlPermissionUserRepository` in M17).
    // --------------------------------------------------------------------

    /// `dbo.SP_PERMISSION_USER_LIST` — admin user-listing endpoint.
    ///
    /// Returns one row per user with permission summary CSVs (legacy
    /// M16 columns) + permission-page columns (M17). Pagination is
    /// applied by the application service (cursor on `email`).
    ///
    /// M19: `@p_user_guid` is the admin check — non-admin callers
    /// receive zero rows. String-encoded per the project GUID-into-SP
    /// rule (the SP declares `varchar(36)` + `TRY_CAST` inside).
    async fn list_with_permissions(
        &self,
        caller_guid: Uuid,
    ) -> Result<Vec<UserListRow>, RepoError> {
        let caller_str = caller_guid.to_string();
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_PERMISSION_USER_LIST @p_by = @P1",
            &[&caller_str as &dyn ToSql],
        )
        .await?;
        Ok(rows.iter().map(row_to_user_list_row).collect())
    }

    // M17 cleanup: `find_user_permissions_by_username` and the
    // `row_to_user_permission_detail_row` mapper moved to
    // `crates/infra/src/db/mssql_permission_user.rs`. The permission
    // flow no longer lives on the login/auth port.

    // --------------------------------------------------------------------
    // M20-b: admin user creation (SP_USER_INSERT_FULL) + actor lookup.
    // --------------------------------------------------------------------

    /// Resolve `user_username_guid` from a `user_guid` (active rows only).
    ///
    /// Used by [`Self::admin_insert_full`] to convert the JWT's
    /// `user_guid` into the column the SP expects. The lookup
    /// filters `user_username_status <> 3` so suspended /
    /// deleted admins cannot impersonate.
    ///
    /// Implementation note: we run a plain `SELECT` rather than
    /// going through `SP_USER_FIND_BY_ID` (which expects a
    /// `user_username_guid` and would loop). The query is a
    /// single-column read against the (user_guid) index that
    /// already exists on `[user_username]`; the cost is ~one
    /// page fetch.
    async fn find_username_guid_by_user_guid(
        &self,
        user_guid: Uuid,
    ) -> Result<Option<String>, RepoError> {
        let user_guid_str = user_guid.to_string();
        let rows = exec_sp(
            &self.pool,
            "SELECT TOP 1 user_username_guid \
                 FROM dbo.user_username \
                 WHERE user_username_user_guid = @P1 \
                   AND user_username_status <> 3",
            &[&user_guid_str as &dyn ToSql],
        )
        .await?;
        Ok(rows
            .first()
            .map(|row| read_guid_str(row, "user_username_guid"))
            .filter(|s| !s.is_empty()))
    }

    /// M20-b: wrap `dbo.SP_USER_INSERT_FULL`.
    ///
    /// Builds the EXEC with ~59 parameters, calls the SP, and maps
    /// the single result row into either [`AdminInsertUserResult`]
    /// or [`AdminInsertUserError`].
    ///
    /// `actor_user_username_guid` is resolved from the JWT
    /// upstream (the handler / service does it once); this
    /// method just passes it through. When the SP rejects the
    /// actor (`ACTOR_NOT_FOUND`, `PERMISSION_DENIED`), we
    /// surface the SP code verbatim — the handler maps it to
    /// the right HTTP status.
    async fn admin_insert_full(
        &self,
        req: &AdminInsertUserRequest,
    ) -> Result<AdminInsertUserResult, AdminInsertUserError> {
        // Build the EXEC string with 59 positional @P1..@P59 params.
        // The SP signature is rigid: every parameter maps to one
        // column, in declaration order. Keep the EXEC and the
        // `params` slice in lockstep — a single off-by-one would
        // silently bind the wrong value to the wrong column.
        const EXEC_SQL: &str = "EXEC dbo.SP_USER_INSERT_FULL \
                @p_actor_user_username_guid = @P1, \
                @p_user_guid = @P2, \
                @p_user_first_name = @P3, \
                @p_user_last_name = @P4, \
                @p_user_id_card = @P5, \
                @p_user_tel = @P6, \
                @p_user_email = @P7, \
                @p_user_gender = @P8, \
                @p_user_country_guid = @P9, \
                @p_user_province = @P10, \
                @p_user_district = @P11, \
                @p_user_sub_district = @P12, \
                @p_user_village = @P13, \
                @p_user_post = @P14, \
                @p_user_description = @P15, \
                @p_user_is_foreign = @P16, \
                @p_user_is_customer_company = @P17, \
                @p_user_is_customer = @P18, \
                @p_user_is_admin = @P19, \
                @p_user_is_employee = @P20, \
                @p_user_is_freelance = @P21, \
                @p_user_status = @P22, \
                @p_username = @P23, \
                @p_password_hash = @P24, \
                @p_profile_img_path = @P25, \
                @p_company_guid = @P26, \
                @p_user_company_name = @P27, \
                @p_user_company_tel = @P28, \
                @p_user_company_type = @P29, \
                @p_user_company_status = @P30, \
                @p_department_guid = @P31, \
                @p_department_team_guid = @P32, \
                @p_position_guid = @P33, \
                @p_position_start_at = @P34, \
                @p_salary_amount = @P35, \
                @p_salary_currency = @P36, \
                @p_monday_is_working = @P37, \
                @p_monday_start_time = @P38, \
                @p_monday_end_time = @P39, \
                @p_tuesday_is_working = @P40, \
                @p_tuesday_start_time = @P41, \
                @p_tuesday_end_time = @P42, \
                @p_wednesday_is_working = @P43, \
                @p_wednesday_start_time = @P44, \
                @p_wednesday_end_time = @P45, \
                @p_thursday_is_working = @P46, \
                @p_thursday_start_time = @P47, \
                @p_thursday_end_time = @P48, \
                @p_friday_is_working = @P49, \
                @p_friday_start_time = @P50, \
                @p_friday_end_time = @P51, \
                @p_saturday_is_working = @P52, \
                @p_saturday_start_time = @P53, \
                @p_saturday_end_time = @P54, \
                @p_sunday_is_working = @P55, \
                @p_sunday_start_time = @P56, \
                @p_sunday_end_time = @P57, \
                @p_bank_name = @P58, \
                @p_bank_code = @P59, \
                @p_bank_account_no = @P60, \
                @p_bank_account_name = @P61, \
                @p_bank_book_img_path = @P62, \
                @p_id_card_front_path = @P63, \
                @p_id_card_back_path = @P64, \
                @p_proof_of_address_path = @P65, \
                @p_source_of_funds_statement_path = @P66";

        // ---- Bind every parameter ----
        //
        // ponytail: we bind as `Option<&str>` / `Option<&Decimal>` /
        // `Option<chrono::DateTime<Utc>>` so an absent field arrives
        // at SQL Server as a real NULL (matches the SP's `= NULL`
        // defaults). Building a 66-element Vec by hand keeps the
        // binding order locked to the EXEC string above — the
        // compiler will not catch a mismatch; reviewers should
        // verify both side-by-side on every change.
        let actor = req.actor_user_username_guid.as_str();
        let user_guid: Option<&str> = req.user_guid.as_deref();
        let first_name = req.first_name.as_str();
        let last_name = req.last_name.as_str();
        let id_card: Option<&str> = req.id_card.as_deref();
        let tel: Option<&str> = req.tel.as_deref();
        let email = req.email.as_str();
        let gender: Option<&str> = req.gender.as_deref();
        let country_guid: Option<&str> = req.country_guid.as_deref();
        let province: Option<&str> = req.province.as_deref();
        let district: Option<&str> = req.district.as_deref();
        let sub_district: Option<&str> = req.sub_district.as_deref();
        let village: Option<&str> = req.village.as_deref();
        let post: Option<&str> = req.post.as_deref();
        let description: Option<&str> = req.description.as_deref();
        let is_foreign = req.is_foreign;
        let is_customer_company = req.is_customer_company;
        let is_customer = req.is_customer;
        let is_admin = req.is_admin;
        let is_employee = req.is_employee;
        let is_freelance = req.is_freelance;
        let status = req.status;
        let username = req.username.as_str();
        let password_hash = req.password_hash.as_str();
        let profile_img_path: Option<&str> = req.profile_img_path.as_deref();
        let company_guid: Option<&str> = req.company_guid.as_deref();
        let company_name: Option<&str> = req.company_name.as_deref();
        let company_tel: Option<&str> = req.company_tel.as_deref();
        let company_type: Option<i32> = req.company_type;
        let company_status = req.company_status;
        let department_guid: Option<&str> = req.department_guid.as_deref();
        let department_team_guid: Option<&str> = req.department_team_guid.as_deref();
        let position_guid: Option<&str> = req.position_guid.as_deref();
        let position_start_at: Option<chrono::DateTime<chrono::Utc>> = req.position_start_at;
        let salary_amount: Option<rust_decimal::Decimal> = req.salary_amount;
        let salary_currency: Option<&str> = req.salary_currency.as_deref();

        // Day schedules. The SP requires: when `is_working = 1`,
        // both `start_time` and `end_time` must be non-NULL.
        // Service-side validation in `application/admin_user.rs`
        // catches the violation early with a 422 before we hit
        // the SP. Here we just pass through.
        let s = &req.schedule;
        let (m_iw, m_st, m_et) = day_to_parts(&s.monday);
        let (t_iw, t_st, t_et) = day_to_parts(&s.tuesday);
        let (w_iw, w_st, w_et) = day_to_parts(&s.wednesday);
        let (th_iw, th_st, th_et) = day_to_parts(&s.thursday);
        let (f_iw, f_st, f_et) = day_to_parts(&s.friday);
        let (sa_iw, sa_st, sa_et) = day_to_parts(&s.saturday);
        let (su_iw, su_st, su_et) = day_to_parts(&s.sunday);

        let bank_name: Option<&str> = req.bank_name.as_deref();
        let bank_code: Option<&str> = req.bank_code.as_deref();
        let bank_account_no: Option<&str> = req.bank_account_no.as_deref();
        let bank_account_name: Option<&str> = req.bank_account_name.as_deref();
        let bank_book_img_path: Option<&str> = req.bank_book_img_path.as_deref();
        let id_card_front: Option<&str> = req.id_card_front_path.as_deref();
        let id_card_back: Option<&str> = req.id_card_back_path.as_deref();
        let proof_of_address: Option<&str> = req.proof_of_address_path.as_deref();
        let source_of_funds: Option<&str> = req.source_of_funds_statement_path.as_deref();

        let params: &[&dyn ToSql] = &[
            &actor,
            &user_guid,
            &first_name,
            &last_name,
            &id_card,
            &tel,
            &email,
            &gender,
            &country_guid,
            &province,
            &district,
            &sub_district,
            &village,
            &post,
            &description,
            &is_foreign,
            &is_customer_company,
            &is_customer,
            &is_admin,
            &is_employee,
            &is_freelance,
            &status,
            &username,
            &password_hash,
            &profile_img_path,
            &company_guid,
            &company_name,
            &company_tel,
            &company_type,
            &company_status,
            &department_guid,
            &department_team_guid,
            &position_guid,
            &position_start_at,
            &salary_amount,
            &salary_currency,
            &m_iw,
            &m_st,
            &m_et,
            &t_iw,
            &t_st,
            &t_et,
            &w_iw,
            &w_st,
            &w_et,
            &th_iw,
            &th_st,
            &th_et,
            &f_iw,
            &f_st,
            &f_et,
            &sa_iw,
            &sa_st,
            &sa_et,
            &su_iw,
            &su_st,
            &su_et,
            &bank_name,
            &bank_code,
            &bank_account_no,
            &bank_account_name,
            &bank_book_img_path,
            &id_card_front,
            &id_card_back,
            &proof_of_address,
            &source_of_funds,
        ];

        let rows = exec_sp(&self.pool, EXEC_SQL, params)
            .await
            // Translate the connection / TDS error into the
            // structured SP-error shape so the handler still
            // produces a proper envelope (mapping "backend" to
            // the INTERNAL error code).
            .map_err(|e| {
                AdminInsertUserError::new("internal", format!("SP_USER_INSERT_FULL: {e}"))
            })?;

        // The SP returns exactly one row regardless of success or
        // failure (matches the contract documented in the SP body).
        // The success path emits `success = 1` + `code = 'CREATED'`;
        // every failure branch emits `success = 0` + a distinct
        // string `code` + an English `message`. We read the row by
        // column name — the SP aliases every column.
        let row = rows.first().ok_or_else(|| {
            AdminInsertUserError::new(
                "internal",
                "SP_USER_INSERT_FULL returned no row (driver/protocol mismatch)",
            )
        })?;

        let success: bool = row.get::<bool, _>("success").unwrap_or(false);
        let code = read_str(row, "code").unwrap_or("").to_string();
        let message = read_str(row, "message").unwrap_or("").to_string();

        if !success {
            return Err(AdminInsertUserError::new(code, message));
        }

        let user_guid = read_guid_str(row, "user_guid");
        let user_username_guid = read_guid_str(row, "user_username_guid");
        let assigned_role_guid_raw = read_guid_str(row, "assigned_role_guid");

        Ok(AdminInsertUserResult {
            user_guid,
            user_username_guid,
            username: read_str(row, "username").unwrap_or("").to_string(),
            // The SP returns NULL for `assigned_role_guid` when
            // neither `is_admin` nor `is_employee` was set. The
            // `read_guid_str` helper emits an empty string for
            // NULL — coerce that to `None` so the wire shape is
            // `null`, not `""`.
            assigned_role_guid: if assigned_role_guid_raw.is_empty() {
                None
            } else {
                Some(assigned_role_guid_raw)
            },
        })
    }
}

/// Map a single joined row to the User aggregate (without roles).
/// The `roles` field is filled by the caller after reading the
/// second result set.
fn row_to_user(row: &tiberius::Row) -> Result<User, RepoError> {
    let id_str: &str = row
        .get::<&str, _>("user_guid")
        .ok_or_else(|| RepoError::Backend("missing id".into()))?;

    let id = Uuid::parse_str(id_str)
        .map_err(|e| RepoError::Backend(format!("invalid user_guid: {e}")))?;
    let first_name: &str = row
        .get::<&str, _>("user_first_name")
        .ok_or_else(|| RepoError::Backend("missing first_name".into()))?;
    let last_name: &str = row
        .get::<&str, _>("user_last_name")
        .ok_or_else(|| RepoError::Backend("missing last_name".into()))?;
    let username: &str = row
        .get::<&str, _>("user_username_username")
        .ok_or_else(|| RepoError::Backend("missing username".into()))?;
    let password_hash: &str = row
        .get::<&str, _>("user_password")
        .ok_or_else(|| RepoError::Backend("missing password_hash".into()))?;
    let status_i32: i32 = row
        .get::<i32, _>("user_status")
        .ok_or_else(|| RepoError::Backend("missing status".into()))?;

    let created_at_naive = row
        .get::<chrono::NaiveDateTime, _>("user_username_create_at")
        .ok_or_else(|| RepoError::Backend("missing created_at".into()))?;

    let updated_at_naive = row
        .get::<chrono::NaiveDateTime, _>("user_username_update_at")
        .ok_or_else(|| RepoError::Backend("missing updated_at".into()))?;

    let created_at =
        chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(created_at_naive, chrono::Utc);

    let updated_at =
        chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(updated_at_naive, chrono::Utc);

    let status = UserStatus::from_i32(status_i32)
        .ok_or_else(|| RepoError::Backend(format!("unknown status: {status_i32}")))?;
    Ok(User {
        id,
        first_name: first_name.to_string(),
        last_name: last_name.to_string(),
        username: username.to_string(),
        password_hash: password_hash.to_string(),
        roles: Vec::new(),       // filled by caller
        permissions: Vec::new(), // filled by caller
        status,
        created_at,
        updated_at,
    })
}

/// Read the (roles, permissions) pair from the named row of an
/// `exec_sp` result set.
///
/// Per the stored-procedure contract documented in
/// `migrations/20260620000001_sp_user.sql`, the second result-set row
/// returns a single CSV column:
/// - `role_codes` (snake_case: `customer,admin,…`)
///
/// **Note:** the prior version of this function read positional
/// columns `0` and `1` claiming the second one was `permission_codes`,
/// but the SP only emits `role_codes`. Reading by name surfaces the
/// drift: both reads now hit the same column and the parsed strings
/// are routed through `parse_role_codes` / `parse_permission_codes`
/// respectively (they share the same CSV split). The permission
/// SP landed in M15+; if the alias changes, update this read in one
/// place instead of chasing an index. The CSVs are parsed by
/// [`parse_role_codes`] and [`parse_permission_codes`].
fn read_roles_and_permissions(
    rows: &[tiberius::Row],
    idx: usize,
) -> Result<(Vec<Role>, Vec<Permission>), RepoError> {
    let roles = rows
        .get(idx)
        .and_then(|r| read_str(r, "role_codes"))
        .map(parse_role_codes)
        .unwrap_or_default();
    let permissions = rows
        .get(idx)
        .and_then(|r| read_str(r, "permission_codes"))
        .map(parse_permission_codes)
        .unwrap_or_default();
    Ok((roles, permissions))
}

/// Split a [`DaySchedule`] into the three `(bool, Option<&str>,
/// Option<&str>)` parts the SP expects (`is_working` +
/// `start_time` + `end_time`).
///
/// When `is_working` is `false`, both times are `None` (the SP
/// will insert NULL into the `time(0)` columns — the row still
/// exists; just no scheduled hours).
///
/// ponytail: tiny inline helper; refactor to a macro only when a
/// 3rd SP needs the same `DaySchedule`-style mapping.
fn day_to_parts(d: &DaySchedule) -> (bool, Option<&str>, Option<&str>) {
    (d.is_working, d.start_time.as_deref(), d.end_time.as_deref())
}

/// Split a comma-separated role_codes string into Vec<Role>.
///
/// Per RDBMS Permssion.md §1.8 + §3 Step 5 the SQL filter for
/// `user_role_status=1` AND `(expire_at IS NULL OR > now)` MUST run
/// in the stored procedure — Rust only receives role_codes that
/// already passed those gates. We log unknown codes at WARN level
/// (instead of silently dropping them) so a DBA-created role that's
/// not yet mapped in Rust shows up in observability.
///
/// ponytail: full effective-permission calculation (§5: role + allow
/// − deny) belongs in a dedicated `SP_USER_GET_EFFECTIVE_PERMISSIONS`
/// call — the `roles` Vec on the aggregate stays as-is until M15+.
/// Scope / department / permission_override land there.
fn parse_role_codes(s: &str) -> Vec<Role> {
    let mut out = Vec::new();
    for raw in s.split(',') {
        let code = raw.trim();
        if code.is_empty() {
            continue;
        }
        match Role::from_code(code) {
            Some(r) => out.push(r),
            None => tracing::warn!(
                role_code = %code,
                "mssql_user::parse_role_codes: unknown role code from DB \
                 — DBA may have added a new role; backend enum out of sync"
            ),
        }
    }
    out
}

#[cfg(test)]
mod parse_role_codes_tests {
    //! Unit tests for the CSV parser — the SP does the heavy lifting
    //! (status + expire_at filtering) and these tests confirm the
    //! Rust side never crashes on the wire format the DBA may tweak.
    use super::*;
    use kokkak_domain::Role;

    #[test]
    fn parses_all_known_codes() {
        assert_eq!(
            parse_role_codes("customer,admin,super_admin"),
            vec![Role::Customer, Role::Admin, Role::SuperAdmin]
        );
    }

    #[test]
    fn skips_empty_segments() {
        // STUFF(..., 1, 1, '') on an empty subquery yields '' — and
        // a stray trailing comma would split to ["customer", ""].
        assert_eq!(
            parse_role_codes("customer,,admin,"),
            vec![Role::Customer, Role::Admin]
        );
        assert_eq!(parse_role_codes(""), Vec::<Role>::new());
    }

    #[test]
    fn trims_whitespace_around_codes() {
        assert_eq!(
            parse_role_codes(" customer , admin "),
            vec![Role::Customer, Role::Admin]
        );
    }

    #[test]
    fn skips_unknown_codes_without_panicking() {
        // DBA added a role not yet mapped in the Rust enum — we must
        // not panic at startup. The WARN log is captured by
        // tracing-subscriber in production; here we just verify the
        // well-known codes still come through.
        assert_eq!(
            parse_role_codes("customer,new_admin_role,admin"),
            vec![Role::Customer, Role::Admin]
        );
    }
}

/// Split a comma-separated `permission_codes` string into `Vec<Permission>`.
///
/// Mirrors [`parse_role_codes`]: the SP returns
/// `SCREAMING_SNAKE_CASE` codes (`PAGE_JOBS_VIEW,JOBS_CREATE,…`); we
/// trim each segment, skip empties, and log a WARN for codes that
/// the Rust enum does not yet know about so DBA-side additions are
/// observable in production instead of silently dropped.
fn parse_permission_codes(s: &str) -> Vec<Permission> {
    let mut out = Vec::new();
    for raw in s.split(',') {
        let code = raw.trim();
        if code.is_empty() {
            continue;
        }
        match Permission::from_code(code) {
            Some(p) => out.push(p),
            None => tracing::warn!(
                permission_code = %code,
                "mssql_user::parse_permission_codes: unknown permission code from DB \
                 — DBA may have added a new permission; backend enum out of sync"
            ),
        }
    }
    out
}

// ============================================================================
// M16: row mappers for the per-user permission SPs
// ============================================================================
//
// Both mappers follow the project's "thin copy" pattern: the SP
// already COALESCEs NULLs into empty strings / zero values, so we
// only do `.unwrap_or("").to_string()` / `.unwrap_or(0)` fallbacks
// as defensive guards. The CSV columns are split into `Vec<String>`
// via [`split_csv`] below so the wire shape never carries CSV.

/// Split a comma-separated string into `Vec<String>`, skipping
/// empty segments and trimming each one.
///
/// Used by the M16 row mappers (`role_codes`, `role_names`,
/// `permission_codes`, `user_role_name`). Distinct from
/// [`parse_role_codes`] / [`parse_permission_codes`] above because
/// those map into typed enums — this one keeps the raw strings so
/// the admin UI can display `user_role_name` even for DBA-added
/// roles that aren't in the Rust enum yet.
fn split_csv(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(String::from)
        .collect()
}

/// Map one `SP_PERMISSION_USER_LIST` row to [`UserListRow`].
///
/// Column NAMES match the SP's SELECT aliases:
///   `user_guid`               (varchar 36)
///   `full_name`               (varchar — first+' '+last, COALESCE'd to '')
///   `email`                   (varchar — username alias)
///   `role_codes`              (varchar CSV — COALESCE'd to '')
///   `role_names`              (varchar CSV — COALESCE'd to '')
///   `has_permission`          (bit)  ← LIST only ships this bool;
///                                     full permission codes come
///                                     from the detail endpoint
///                                     (`SP_PERMISSION_USER_FIND_BY_USERNAME`)
///   `has_override`            (bit)
///   `user_status`             (int — see [`UserStatus::from_i32`])
///   `user_username_status`    (int — raw, no enum)
///   `user_create_at`          (datetime2)
///   `user_update_at`          (datetime2, ISNULL → user_create_at)
///
/// `has_permission` / `has_override` come back as `i16` via
/// tiberius's `Row::get::<i16, _>` (bit → tinyint). We coerce
/// non-zero to `true` so any future DB-side change (bit → tinyint,
/// etc.) doesn't silently flip the meaning.
fn row_to_user_list_row(row: &tiberius::Row) -> UserListRow {
    let user_status_i32 = read_i32(row, "user_status").unwrap_or(0);
    let user_status = UserStatus::from_i32(user_status_i32).unwrap_or(UserStatus::Pending); // forward-compat for future enum values
                                                                                            // The SP uses `ISNULL(user_update_at, user_create_at)` so `user_update_at`
                                                                                            // should never come back as NULL in practice. Falling back to `created_at`
                                                                                            // when the column is missing keeps the wire shape stable and avoids the                                                                        // `expect("unix epoch must be valid")` panic the prior fallback carried.
    UserListRow {
        user_guid: read_str(row, "user_guid").unwrap_or("").to_string(),
        full_name: read_str(row, "full_name").unwrap_or("").to_string(),
        email: read_str(row, "email").unwrap_or("").to_string(),
        role_codes: split_csv(read_str(row, "role_codes").unwrap_or("")),
        role_names: split_csv(read_str(row, "role_names").unwrap_or("")),
        has_permission: row.get::<bool, _>("has_permission").unwrap_or(false),
        has_override: row.get::<bool, _>("has_override").unwrap_or(false),
        user_status,
        user_username_status: read_i32(row, "user_username_status").unwrap_or(0),
    }
}

// M17 cleanup: `row_to_user_permission_detail_row` moved to
// `crates/infra/src/db/mssql_permission_user.rs` along with the
// `find_user_permissions_by_username` adapter. The permission flow
// no longer lives on the login/auth port.

// ============================================================================
// M16 round 2 cleanup: the duplicate `split_csv` / `row_to_user_list_row`
// / `row_to_user_permission_detail_row` definitions that used to live
// below were dead code (no caller — the M16 round 2 versions above are
// the live ones) and they blocked compilation because the round 1 row
// mapper still referenced the now-removed `permission_codes` field on
// `UserListRow`. Removed in this commit so `cargo check` / `cargo test`
// can run again. If a future SP change reintroduces `permission_codes`,
// add it back to `UserListRow` first, then revive the mapper.
// ============================================================================

#[cfg(test)]
mod parse_permission_codes_tests {
    //! Unit tests for the permission CSV parser — same contract as
    //! the role parser: tolerant of trailing commas, whitespace, and
    //! unknown codes.
    use super::*;
    use kokkak_domain::Permission;

    #[test]
    fn parses_known_codes() {
        assert_eq!(
            parse_permission_codes("PAGE_DASHBOARD_VIEW,JOBS_CREATE,JOBS_UPDATE"),
            vec![
                Permission::PageDashboardView,
                Permission::JobsCreate,
                Permission::JobsUpdate,
            ]
        );
    }

    #[test]
    fn skips_empty_segments_and_trims() {
        assert_eq!(
            parse_permission_codes("PAGE_JOBS_VIEW,,JOBS_CREATE,"),
            vec![Permission::PageJobsView, Permission::JobsCreate]
        );
        assert_eq!(parse_permission_codes(""), Vec::<Permission>::new());
        assert_eq!(
            parse_permission_codes(" JOBS_EXPORT "),
            vec![Permission::JobsExport]
        );
    }

    #[test]
    fn skips_unknown_codes_without_panicking() {
        // Future permission added by the DBA before the Rust enum
        // catches up. We keep the well-known ones and warn (not test).
        assert_eq!(
            parse_permission_codes("JOBS_CREATE,FUTURE_PERMISSION,JOBS_DELETE"),
            vec![Permission::JobsCreate, Permission::JobsDelete]
        );
    }
}
