use async_trait::async_trait;
use tiberius::ToSql;

use kokkak_domain::admin_user::{
    AdminDeleteUserError, AdminDeleteUserResult, AdminInsertUserError, AdminInsertUserRequest,
    AdminInsertUserResult, AdminUpdateUserError, AdminUpdateUserRequest, AdminUpdateUserResult,
    AdminUserDetail, AdminUserDetailAttachment, AdminUserDetailBankAccount, AdminUserDetailCompany,
    AdminUserDetailCountry, AdminUserDetailPosition, AdminUserDetailProfileImage,
    AdminUserDetailRoles, AdminUserDetailSalary, AdminUserDetailScope, AdminUserDetailUsername,
    AdminUserListPagingInput, AdminUserListPagingPage, DaySchedule, UserListPagingRow,
    WeeklySchedule,
};
use kokkak_domain::{Permission, RepoError, Role, User, UserListRow, UserRepository, UserStatus};
use uuid::Uuid;

use crate::db::mssql::{
    exec_sp, read_datetime, read_decimal, read_guid_str, read_i32, read_str, MssqlPool, SpError,
};

#[derive(Clone)]
pub struct MssqlUserRepository {
    pool: MssqlPool,
}

impl MssqlUserRepository {
    pub fn new(pool: MssqlPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserRepository for MssqlUserRepository {
    async fn find_by_id(&self, id: Uuid) -> Result<Option<User>, RepoError> {
        let id_str = id.to_string();
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_USER_FIND_BY_ID @p_user_guid = @P1",
            &[&id_str as &dyn ToSql],
        )
        .await?;

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

        let profile = match rows.first() {
            None => return Ok(None),
            Some(r) => r,
        };
        let user = row_to_user(profile)?;

        let (roles, permissions) = read_roles_and_permissions(&rows, 1)?;
        Ok(Some(User {
            roles,
            permissions,
            ..user
        }))
    }

    async fn insert(&self, user: &User) -> Result<(), RepoError> {
        let role_code = user
            .roles
            .first()
            .map(|r| r.as_str())
            .ok_or_else(|| RepoError::Backend("at least one role required".into()))?;

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
            let _ = set_rows;
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

    async fn find_username_guid_by_user_guid(
        &self,
        user_guid: Uuid,
    ) -> Result<Option<String>, RepoError> {
        let user_guid_str = user_guid.to_string();
        let rows = exec_sp(
            &self.pool,
            "SELECT TOP 1 user_username_guid \
                 FROM dbo.user_username \
                 WHERE user_username_guid = @P1 \
                   AND user_username_status <> 3",
            &[&user_guid_str as &dyn ToSql],
        )
        .await?;
        Ok(rows
            .first()
            .map(|row| read_guid_str(row, "user_username_guid"))
            .filter(|s| !s.is_empty()))
    }

    async fn admin_insert_full(
        &self,
        req: &AdminInsertUserRequest,
    ) -> Result<AdminInsertUserResult, AdminInsertUserError> {
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

        let rows = exec_sp(&self.pool, EXEC_SQL, params).await.map_err(|e| {
            AdminInsertUserError::new("internal", format!("SP_USER_INSERT_FULL: {e}"))
        })?;

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

            assigned_role_guid: if assigned_role_guid_raw.is_empty() {
                None
            } else {
                Some(assigned_role_guid_raw)
            },
        })
    }

    async fn admin_update_full(
        &self,
        req: &AdminUpdateUserRequest,
    ) -> Result<AdminUpdateUserResult, AdminUpdateUserError> {
        const EXEC_SQL: &str = "EXEC dbo.SP_USER_UPDATE_FULL \
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
                        @p_profile_img_path = @P24, \
                        @p_company_guid = @P25, \
                        @p_user_company_name = @P26, \
                        @p_user_company_tel = @P27, \
                        @p_user_company_type = @P28, \
                        @p_user_company_status = @P29, \
                        @p_department_guid = @P30, \
                        @p_department_team_guid = @P31, \
                        @p_position_guid = @P32, \
                        @p_position_start_at = @P33, \
                        @p_salary_amount = @P34, \
                        @p_salary_currency = @P35, \
                        @p_monday_is_working = @P36, \
                        @p_monday_start_time = @P37, \
                        @p_monday_end_time = @P38, \
                        @p_tuesday_is_working = @P39, \
                        @p_tuesday_start_time = @P40, \
                        @p_tuesday_end_time = @P41, \
                        @p_wednesday_is_working = @P42, \
                        @p_wednesday_start_time = @P43, \
                        @p_wednesday_end_time = @P44, \
                        @p_thursday_is_working = @P45, \
                        @p_thursday_start_time = @P46, \
                        @p_thursday_end_time = @P47, \
                        @p_friday_is_working = @P48, \
                        @p_friday_start_time = @P49, \
                        @p_friday_end_time = @P50, \
                        @p_saturday_is_working = @P51, \
                        @p_saturday_start_time = @P52, \
                        @p_saturday_end_time = @P53, \
                        @p_sunday_is_working = @P54, \
                        @p_sunday_start_time = @P55, \
                        @p_sunday_end_time = @P56, \
                        @p_bank_name = @P57, \
                        @p_bank_code = @P58, \
                        @p_bank_account_no = @P59, \
                        @p_bank_account_name = @P60, \
                        @p_bank_book_img_path = @P61, \
                        @p_id_card_front_path = @P62, \
                        @p_id_card_back_path = @P63, \
                        @p_proof_of_address_path = @P64, \
                        @p_source_of_funds_statement_path = @P65";

        let actor = req.actor_user_username_guid.as_str();
        let user_guid = req.user_guid.as_str();
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

        let rows = exec_sp(&self.pool, EXEC_SQL, params).await.map_err(|e| {
            AdminUpdateUserError::new("internal", format!("SP_USER_UPDATE_FULL: {e}"))
        })?;

        let row = rows.first().ok_or_else(|| {
            AdminUpdateUserError::new(
                "internal",
                "SP_USER_UPDATE_FULL returned no row (driver/protocol mismatch)",
            )
        })?;

        let success: bool = row.get::<bool, _>("success").unwrap_or(false);
        let code = read_str(row, "code").unwrap_or("").to_string();
        let message = read_str(row, "message").unwrap_or("").to_string();

        if !success {
            return Err(AdminUpdateUserError::new(code, message));
        }

        Ok(AdminUpdateUserResult {
            user_guid: read_guid_str(row, "user_guid"),
        })
    }

    async fn list_users_paging(
        &self,
        input: &AdminUserListPagingInput,
        actor: Uuid,
    ) -> Result<AdminUserListPagingPage, RepoError> {
        let _ = actor;

        let page = input.page as i32;
        let page_size = input.page_size as i32;

        let status_filter: Option<String> = input.user_status.map(|s| s.to_string());
        let keyword = input.keyword.clone();

        let user_is_customer = input.user_is_customer;
        let user_is_employee = input.user_is_employee;
        let user_is_freelance = input.user_is_freelance;

        let department_guid = input.department_guid.as_deref();
        let department_team_guid = input.department_team_guid.as_deref();
        let position_guid = input.position_guid.as_deref();

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_USER_LIST_PAGING \
                                     @p_keyword = @P1, \
                                     @p_user_status = @P2, \
                                     @p_user_is_customer = @P3, \
                                     @p_user_is_employee = @P4, \
                                     @p_user_is_freelance = @P5, \
                                     @p_department_guid = @P6, \
                                     @p_department_team_guid = @P7, \
                                     @p_position_guid = @P8, \
                                     @p_page = @P9, \
                                     @p_page_size = @P10",
            &[
                &keyword as &dyn ToSql,
                &status_filter.as_deref() as &dyn ToSql,
                &user_is_customer as &dyn ToSql,
                &user_is_employee as &dyn ToSql,
                &user_is_freelance as &dyn ToSql,
                &department_guid as &dyn ToSql,
                &department_team_guid as &dyn ToSql,
                &position_guid as &dyn ToSql,
                &page as &dyn ToSql,
                &page_size as &dyn ToSql,
            ],
        )
        .await?;

        let items: Vec<UserListPagingRow> = rows.iter().map(row_to_user_list_paging_row).collect();

        let (total_count, out_page, out_page_size) = items
            .first()
            .map(|r| (r.total_count, r.page, r.page_size))
            .unwrap_or((0, page, page_size));

        Ok(AdminUserListPagingPage {
            items,
            total_count,
            page: out_page,
            page_size: out_page_size,
        })
    }

    async fn get_user_detail_full(
        &self,
        user_guid: Uuid,
        actor: Uuid,
    ) -> Result<Option<AdminUserDetail>, RepoError> {
        let _ = actor;

        let user_guid_str = user_guid.to_string();
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_USER_DETAIL_FULL_GET @p_user_guid = @P1",
            &[&user_guid_str as &dyn ToSql],
        )
        .await?;

        let row = match rows.first() {
            None => return Ok(None),
            Some(r) => r,
        };
        Ok(Some(row_to_admin_user_detail(row)))
    }

    async fn admin_delete_user(
        &self,
        actor_user_username_guid: &str,
        user_guid: &str,
    ) -> Result<AdminDeleteUserResult, AdminDeleteUserError> {
        const EXEC_SQL: &str = "EXEC dbo.SP_USER_DELETE \
                @p_actor_user_username_guid = @P1, \
                @p_user_guid = @P2";

        let params: &[&dyn ToSql] = &[&actor_user_username_guid, &user_guid];

        let rows = exec_sp(&self.pool, EXEC_SQL, params)
            .await
            .map_err(|e| AdminDeleteUserError::new("internal", format!("SP_USER_DELETE: {e}")))?;

        let row = rows.first().ok_or_else(|| {
            AdminDeleteUserError::new(
                "internal",
                "SP_USER_DELETE returned no row (driver/protocol mismatch)",
            )
        })?;

        let success: bool = row.get::<bool, _>("success").unwrap_or(false);
        let code = read_str(row, "code").unwrap_or("").to_string();
        let message = read_str(row, "message").unwrap_or("").to_string();
        let result_user_guid = read_guid_str(row, "user_guid");

        if !success {
            return Err(AdminDeleteUserError::new(code, message));
        }

        Ok(AdminDeleteUserResult {
            user_guid: result_user_guid,
            code,
            message,
        })
    }

    async fn admin_suspend_user(
        &self,
        actor_user_username_guid: &str,
        user_guid: &str,
    ) -> Result<AdminDeleteUserResult, AdminDeleteUserError> {
        const EXEC_SQL: &str = "EXEC dbo.SP_USER_SUSPEND \
                @p_actor_user_username_guid = @P1, \
                @p_user_guid = @P2";

        let params: &[&dyn ToSql] = &[&actor_user_username_guid, &user_guid];

        let rows = exec_sp(&self.pool, EXEC_SQL, params)
            .await
            .map_err(|e| AdminDeleteUserError::new("internal", format!("SP_USER_SUSPEND: {e}")))?;

        let row = rows.first().ok_or_else(|| {
            AdminDeleteUserError::new(
                "internal",
                "SP_USER_SUSPEND returned no row (driver/protocol mismatch)",
            )
        })?;

        let success: bool = row.get::<bool, _>("success").unwrap_or(false);
        let code = read_str(row, "code").unwrap_or("").to_string();
        let message = read_str(row, "message").unwrap_or("").to_string();
        let result_user_guid = read_guid_str(row, "user_guid");

        if !success {
            return Err(AdminDeleteUserError::new(code, message));
        }

        Ok(AdminDeleteUserResult {
            user_guid: result_user_guid,
            code,
            message,
        })
    }
}

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
        roles: Vec::new(),
        permissions: Vec::new(),
        status,
        created_at,
        updated_at,
    })
}

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

fn day_to_parts(d: &DaySchedule) -> (bool, Option<String>, Option<String>) {
    let fmt = |t: chrono::NaiveTime| t.format("%H:%M:%S").to_string();
    (d.is_working, d.start_time.map(fmt), d.end_time.map(fmt))
}

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
        assert_eq!(
            parse_role_codes("customer,new_admin_role,admin"),
            vec![Role::Customer, Role::Admin]
        );
    }
}

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

fn split_csv(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(String::from)
        .collect()
}

fn row_to_user_list_row(row: &tiberius::Row) -> UserListRow {
    let user_status_i32 = read_i32(row, "user_status").unwrap_or(0);
    let user_status = UserStatus::from_i32(user_status_i32).unwrap_or(UserStatus::Pending);

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

fn row_to_user_list_paging_row(row: &tiberius::Row) -> UserListPagingRow {
    UserListPagingRow {
        total_count: read_i32(row, "total_count").unwrap_or(0) as i64,
        page: read_i32(row, "page").unwrap_or(1),
        page_size: read_i32(row, "page_size").unwrap_or(20),
        user_guid: read_guid_str(row, "user_guid"),
        full_name: read_str(row, "full_name").unwrap_or("").to_string(),
        phone: read_str(row, "phone").unwrap_or("").to_string(),
        user_status: read_i32(row, "user_status").unwrap_or(0),
        user_status_name: read_str(row, "user_status_name").unwrap_or("").to_string(),

        user_is_customer: row.get::<bool, _>("user_is_customer").unwrap_or(false),
        user_is_employee: row.get::<bool, _>("user_is_employee").unwrap_or(false),
        user_is_freelance: row.get::<bool, _>("user_is_freelance").unwrap_or(false),
        role_name: read_str(row, "role_name").unwrap_or("").to_string(),
        department_guid: read_guid_str(row, "department_guid"),
        department_name: read_str(row, "department_name").unwrap_or("").to_string(),
        department_team_guid: read_guid_str(row, "department_team_guid"),
        department_team_name: read_str(row, "department_team_name")
            .unwrap_or("")
            .to_string(),
        position_guid: read_guid_str(row, "position_guid"),
        position_name: read_str(row, "position_name").unwrap_or("").to_string(),
    }
}

fn row_to_username(row: &tiberius::Row) -> Option<AdminUserDetailUsername> {
    let guid = read_guid_str(row, "user_username_guid");
    if guid.is_empty() {
        return None;
    }
    Some(AdminUserDetailUsername {
        user_username_guid: guid,
        username: read_str(row, "user_username_username")
            .unwrap_or("")
            .to_string(),
        status: read_i32(row, "user_username_status").unwrap_or(0),
        created_at: read_datetime(row, "user_username_create_at"),
        updated_at: read_datetime(row, "user_username_update_at"),
    })
}

fn row_to_profile_image(row: &tiberius::Row) -> Option<AdminUserDetailProfileImage> {
    let guid = read_guid_str(row, "user_img_profile_guid");
    if guid.is_empty() {
        return None;
    }
    Some(AdminUserDetailProfileImage {
        user_img_profile_guid: guid,
        profile_img_path: read_str(row, "profile_img_path").unwrap_or("").to_string(),

        profile_img_url: None,
    })
}

fn row_to_country(row: &tiberius::Row) -> Option<AdminUserDetailCountry> {
    let guid = read_guid_str(row, "master_country_guid");
    if guid.is_empty() {
        return None;
    }
    Some(AdminUserDetailCountry {
        country_guid: guid,
        country_code: read_str(row, "country_code").unwrap_or("").to_string(),
        country_name: read_str(row, "country_name").unwrap_or("").to_string(),
    })
}

fn row_to_company(row: &tiberius::Row) -> Option<AdminUserDetailCompany> {
    let guid = read_guid_str(row, "user_company_guid");
    if guid.is_empty() {
        return None;
    }
    Some(AdminUserDetailCompany {
        user_company_guid: guid,
        company_guid: read_guid_str(row, "company_guid"),
        company_name: read_str(row, "company_name").unwrap_or("").to_string(),
        company_tel: read_str(row, "company_tel").unwrap_or("").to_string(),
        user_company_name: read_str(row, "user_company_name").unwrap_or("").to_string(),
        user_company_tel: read_str(row, "user_company_tel").unwrap_or("").to_string(),
        user_company_type: read_i32(row, "user_company_type").unwrap_or(0),
        user_company_status: read_i32(row, "user_company_status").unwrap_or(0),
    })
}

fn row_to_roles(row: &tiberius::Row) -> Option<AdminUserDetailRoles> {
    let codes = read_str(row, "role_codes").unwrap_or("");
    let names = read_str(row, "role_names").unwrap_or("");
    if codes.is_empty() && names.is_empty() && !row_has_col(row, "user_is_admin") {
        return None;
    }
    Some(AdminUserDetailRoles {
        role_codes: codes.to_string(),
        role_names: names.to_string(),
        user_is_admin: row.get::<bool, _>("user_is_admin").unwrap_or(false),
    })
}

fn row_to_scope(row: &tiberius::Row) -> Option<AdminUserDetailScope> {
    let dept_guid = read_guid_str(row, "department_guid");
    let team_guid = read_guid_str(row, "department_team_guid");
    if dept_guid.is_empty() && team_guid.is_empty() {
        return None;
    }
    Some(AdminUserDetailScope {
        department_guid: dept_guid,
        department_code: read_str(row, "department_code").unwrap_or("").to_string(),
        department_name: read_str(row, "department_name").unwrap_or("").to_string(),
        department_team_guid: team_guid,
        department_team_code: read_str(row, "department_team_code")
            .unwrap_or("")
            .to_string(),
        department_team_name: read_str(row, "department_team_name")
            .unwrap_or("")
            .to_string(),
    })
}

fn row_to_position(row: &tiberius::Row) -> Option<AdminUserDetailPosition> {
    let guid = read_guid_str(row, "user_position_guid");
    if guid.is_empty() {
        return None;
    }
    Some(AdminUserDetailPosition {
        user_position_guid: guid,
        position_guid: read_guid_str(row, "position_guid"),
        position_code: read_str(row, "position_code").unwrap_or("").to_string(),
        position_name: read_str(row, "position_name").unwrap_or("").to_string(),

        position_level: read_i32(row, "position_level").unwrap_or(0),
        position_start_at: read_datetime(row, "position_start_at"),
        position_end_at: read_datetime(row, "position_end_at"),
    })
}

fn row_to_salary(row: &tiberius::Row) -> Option<AdminUserDetailSalary> {
    let guid = read_guid_str(row, "user_salary_guid");
    if guid.is_empty() {
        return None;
    }
    Some(AdminUserDetailSalary {
        user_salary_guid: guid,

        salary_amount: read_decimal(row, "salary_amount").unwrap_or_default(),
        salary_currency: read_str(row, "salary_currency").unwrap_or("").to_string(),
        salary_type: read_i32(row, "salary_type").unwrap_or(0),
        salary_effective_from: read_datetime(row, "salary_effective_from"),
        salary_effective_to: read_datetime(row, "salary_effective_to"),
    })
}

fn row_to_schedule(row: &tiberius::Row) -> Option<WeeklySchedule> {
    let guid = read_guid_str(row, "user_work_day_template_guid");
    if guid.is_empty() {
        return None;
    }

    Some(WeeklySchedule {
        monday: DaySchedule {
            is_working: row.get::<bool, _>("monday_is_working").unwrap_or(false),
            start_time: row.get::<chrono::NaiveTime, _>("monday_start_time"),
            end_time: row.get::<chrono::NaiveTime, _>("monday_end_time"),
        },
        tuesday: DaySchedule {
            is_working: row.get::<bool, _>("tuesday_is_working").unwrap_or(false),
            start_time: row.get::<chrono::NaiveTime, _>("tuesday_start_time"),
            end_time: row.get::<chrono::NaiveTime, _>("tuesday_end_time"),
        },
        wednesday: DaySchedule {
            is_working: row.get::<bool, _>("wednesday_is_working").unwrap_or(false),
            start_time: row.get::<chrono::NaiveTime, _>("wednesday_start_time"),
            end_time: row.get::<chrono::NaiveTime, _>("wednesday_end_time"),
        },
        thursday: DaySchedule {
            is_working: row.get::<bool, _>("thursday_is_working").unwrap_or(false),
            start_time: row.get::<chrono::NaiveTime, _>("thursday_start_time"),
            end_time: row.get::<chrono::NaiveTime, _>("thursday_end_time"),
        },
        friday: DaySchedule {
            is_working: row.get::<bool, _>("friday_is_working").unwrap_or(false),
            start_time: row.get::<chrono::NaiveTime, _>("friday_start_time"),
            end_time: row.get::<chrono::NaiveTime, _>("friday_end_time"),
        },
        saturday: DaySchedule {
            is_working: row.get::<bool, _>("saturday_is_working").unwrap_or(false),
            start_time: row.get::<chrono::NaiveTime, _>("saturday_start_time"),
            end_time: row.get::<chrono::NaiveTime, _>("saturday_end_time"),
        },
        sunday: DaySchedule {
            is_working: row.get::<bool, _>("sunday_is_working").unwrap_or(false),
            start_time: row.get::<chrono::NaiveTime, _>("sunday_start_time"),
            end_time: row.get::<chrono::NaiveTime, _>("sunday_end_time"),
        },
    })
}

fn row_to_bank_account(row: &tiberius::Row) -> Option<AdminUserDetailBankAccount> {
    let guid = read_guid_str(row, "user_bank_account_guid");
    if guid.is_empty() {
        return None;
    }
    Some(AdminUserDetailBankAccount {
        user_bank_account_guid: guid,
        bank_name: read_str(row, "bank_name").unwrap_or("").to_string(),
        bank_code: read_str(row, "bank_code").unwrap_or("").to_string(),
        branch_name: read_str(row, "branch_name").unwrap_or("").to_string(),
        bank_account_name: read_str(row, "bank_account_name").unwrap_or("").to_string(),
        bank_account_no: read_str(row, "bank_account_no").unwrap_or("").to_string(),
        bank_account_no_masked: read_str(row, "bank_account_no_masked")
            .unwrap_or("")
            .to_string(),
        bank_account_type: read_i32(row, "bank_account_type").unwrap_or(0),
        bank_account_is_default: row
            .get::<bool, _>("bank_account_is_default")
            .unwrap_or(false),
        bank_account_verified_status: read_i32(row, "bank_account_verified_status").unwrap_or(0),
        bank_book_img_path: read_str(row, "bank_book_img_path")
            .unwrap_or("")
            .to_string(),

        bank_book_img_url: None,
    })
}

fn row_to_attachment(
    row: &tiberius::Row,
    guid_col: &str,
    path_col: &str,
) -> Option<AdminUserDetailAttachment> {
    let guid = read_guid_str(row, guid_col);
    if guid.is_empty() {
        return None;
    }
    Some(AdminUserDetailAttachment {
        user_details_attachment_guid: guid,
        attachment_path: read_str(row, path_col).unwrap_or("").to_string(),

        attachment_url: None,
    })
}

fn row_has_col(row: &tiberius::Row, col: &str) -> bool {
    if let Ok(Some(_)) = row.try_get::<&str, _>(col) {
        return true;
    }
    if let Ok(Some(_)) = row.try_get::<bool, _>(col) {
        return true;
    }
    if let Ok(Some(_)) = row.try_get::<i32, _>(col) {
        return true;
    }
    if let Ok(Some(_)) = row.try_get::<i64, _>(col) {
        return true;
    }
    false
}

fn row_to_admin_user_detail(row: &tiberius::Row) -> AdminUserDetail {
    AdminUserDetail {
        user_guid: read_guid_str(row, "user_guid"),
        user_first_name: read_str(row, "user_first_name").unwrap_or("").to_string(),
        user_last_name: read_str(row, "user_last_name").unwrap_or("").to_string(),
        full_name: read_str(row, "full_name").unwrap_or("").to_string(),
        user_id_card: read_str(row, "user_id_card").unwrap_or("").to_string(),
        user_tel: read_str(row, "user_tel").unwrap_or("").to_string(),
        user_email: read_str(row, "user_email").unwrap_or("").to_string(),
        user_gender: read_str(row, "user_gender").unwrap_or("").to_string(),

        user_is_foreign: row.get::<bool, _>("user_is_foreign").unwrap_or(false),
        user_country_guid: read_guid_str(row, "user_country_guid"),

        user_province: read_str(row, "user_province").unwrap_or("").to_string(),
        user_district: read_str(row, "user_district").unwrap_or("").to_string(),
        user_sub_district: read_str(row, "user_sub_district").unwrap_or("").to_string(),
        user_village: read_str(row, "user_village").unwrap_or("").to_string(),
        user_post: read_str(row, "user_post").unwrap_or("").to_string(),
        user_description: read_str(row, "user_description").unwrap_or("").to_string(),

        user_is_customer_company: row
            .get::<bool, _>("user_is_customer_company")
            .unwrap_or(false),
        user_is_customer: row.get::<bool, _>("user_is_customer").unwrap_or(false),
        user_is_employee: row.get::<bool, _>("user_is_employee").unwrap_or(false),
        user_is_freelance: row.get::<bool, _>("user_is_freelance").unwrap_or(false),
        user_is_admin: row.get::<bool, _>("user_is_admin").unwrap_or(false),

        user_status: read_i32(row, "user_status").unwrap_or(0),
        user_status_name: read_str(row, "user_status_name").unwrap_or("").to_string(),

        user_create_at: read_datetime(row, "user_create_at"),
        user_create_by: read_str(row, "user_create_by").unwrap_or("").to_string(),
        user_update_at: read_datetime(row, "user_update_at"),
        user_update_by: read_str(row, "user_update_by").unwrap_or("").to_string(),

        username: row_to_username(row),
        profile_image: row_to_profile_image(row),
        country: row_to_country(row),
        company: row_to_company(row),
        roles: row_to_roles(row),
        scope: row_to_scope(row),
        position: row_to_position(row),
        salary: row_to_salary(row),
        schedule: row_to_schedule(row),
        user_work_day_template_guid: read_guid_str(row, "user_work_day_template_guid"),
        bank_account: row_to_bank_account(row),

        id_card_front: row_to_attachment(row, "id_card_front_guid", "id_card_front_path"),
        id_card_back: row_to_attachment(row, "id_card_back_guid", "id_card_back_path"),
        proof_of_address: row_to_attachment(row, "proof_of_address_guid", "proof_of_address_path"),
        source_of_funds_statement: row_to_attachment(
            row,
            "source_of_funds_statement_guid",
            "source_of_funds_statement_path",
        ),
    }
}

#[cfg(test)]
mod parse_permission_codes_tests {

    use super::*;
    use kokkak_domain::Permission;

    #[test]
    fn parses_known_codes() {
        assert_eq!(
            parse_permission_codes("DASHBOARD_VIEW,JOBS_CREATE,JOBS_UPDATE"),
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
            parse_permission_codes("JOBS_VIEW,,JOBS_CREATE,"),
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
        assert_eq!(
            parse_permission_codes("JOBS_CREATE,FUTURE_PERMISSION,JOBS_DELETE"),
            vec![Permission::JobsCreate, Permission::JobsDelete]
        );
    }
}
