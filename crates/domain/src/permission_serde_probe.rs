// TEMPORARY PROBE — remove after confirming wire format
#[cfg(test)]
mod probe {
    use crate::Permission;

    #[test]
    fn debug_permission_serde_roundtrip() {
        for p in [
            Permission::UsersCreate,
            Permission::PagePermissionsView,
            Permission::JobsCreate,
            Permission::PageDashboardView,
        ] {
            let s = serde_json::to_string(&p).unwrap();
            let round: Permission = serde_json::from_str(&s).unwrap();
            assert_eq!(p, round, "round-trip failed for {p:?}");
            eprintln!("  {p:?}  →  wire={s}  →  round={round:?}");
        }
    }
}
