use kokkak_domain::StorageKey;
use uuid::Uuid;

pub fn user_profile(user_guid: &str, ext: &str) -> StorageKey {
    StorageKey(format!(
        "users/{}/profile/{}.{}",
        user_guid,
        Uuid::now_v7(),
        normalize_ext(ext)
    ))
}

pub fn user_bank_book(user_guid: &str, ext: &str) -> StorageKey {
    StorageKey(format!(
        "users/{}/bank-book/{}.{}",
        user_guid,
        Uuid::now_v7(),
        normalize_ext(ext)
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserAttachment {
    IdCardFront,

    IdCardBack,

    ProofOfAddress,

    SourceOfFunds,
}

impl UserAttachment {
    fn folder(self) -> &'static str {
        match self {
            UserAttachment::IdCardFront => "id-card-front",
            UserAttachment::IdCardBack => "id-card-back",
            UserAttachment::ProofOfAddress => "proof-of-address",
            UserAttachment::SourceOfFunds => "source-of-funds",
        }
    }
}

pub fn user_attachment(user_guid: &str, kind: UserAttachment, ext: &str) -> StorageKey {
    StorageKey(format!(
        "users/{}/attachments/{}/{}.{}",
        user_guid,
        kind.folder(),
        Uuid::now_v7(),
        normalize_ext(ext)
    ))
}

pub fn category_job_main_icon<S: AsRef<str>>(category_guid: S, ext: &str) -> StorageKey {
    StorageKey(format!(
        "category-job-mains/{}/icon/{}.{}",
        category_guid.as_ref(),
        Uuid::now_v7(),
        normalize_ext(ext)
    ))
}

pub fn category_job_service_main_icon<S: AsRef<str>>(service_guid: S, ext: &str) -> StorageKey {
    StorageKey(format!(
        "category-job-services/{}/icon/{}.{}",
        service_guid.as_ref(),
        Uuid::now_v7(),
        normalize_ext(ext)
    ))
}

pub fn category_job_service_sub_image<S: AsRef<str>>(service_sub_guid: S, ext: &str) -> StorageKey {
    StorageKey(format!(
        "category-job-service-subs/{}/image/{}.{}",
        service_sub_guid.as_ref(),
        Uuid::now_v7(),
        normalize_ext(ext)
    ))
}

fn normalize_ext(ext: &str) -> String {
    ext.trim().trim_start_matches('.').to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_key_layout() {
        let k = user_profile("11111111-2222-3333-4444-555555555555", "jpg");
        let s = k.as_str();
        assert!(s.starts_with("users/11111111-2222-3333-4444-555555555555/profile/"));
        assert!(s.ends_with(".jpg"));
    }

    #[test]
    fn bank_book_key_layout() {
        let k = user_bank_book("u-1", "png");
        assert_eq!(
            k.as_str()
                .strip_prefix("users/u-1/bank-book/")
                .map(|_| true),
            Some(true)
        );
        assert!(k.as_str().ends_with(".png"));
    }

    #[test]
    fn attachment_keys_cover_all_kinds() {
        for (i, kind) in [
            UserAttachment::IdCardFront,
            UserAttachment::IdCardBack,
            UserAttachment::ProofOfAddress,
            UserAttachment::SourceOfFunds,
        ]
        .into_iter()
        .enumerate()
        {
            let k = user_attachment("u-1", kind, "jpg");

            assert!(k.as_str().contains(kind.folder()), "kind #{i}");
        }
    }

    #[test]
    fn extension_is_normalized() {
        let k = user_profile("u", ".JPG");
        assert!(k.as_str().ends_with(".jpg"));
        let k = user_profile("u", "  png  ");
        assert!(k.as_str().ends_with(".png"));
    }

    #[test]
    fn uuids_are_unique() {
        let a = user_profile("u", "jpg");
        let b = user_profile("u", "jpg");
        assert_ne!(a.as_str(), b.as_str());
    }

    #[test]
    fn category_job_main_icon_key_layout() {
        let k = category_job_main_icon("11111111-2222-3333-4444-555555555555", "png");
        let s = k.as_str();
        assert!(s.starts_with("category-job-mains/11111111-2222-3333-4444-555555555555/icon/"));
        assert!(s.ends_with(".png"));
    }

    #[test]
    fn category_job_service_main_icon_key_layout() {
        let k = category_job_service_main_icon("22222222-3333-4444-5555-666666666666", "webp");
        let s = k.as_str();
        assert!(s.starts_with("category-job-services/22222222-3333-4444-5555-666666666666/icon/"));
        assert!(s.ends_with(".webp"));
    }

    #[test]
    fn category_job_service_sub_image_key_layout() {
        let k = category_job_service_sub_image("33333333-4444-5555-6666-777777777777", "webp");
        let s = k.as_str();
        assert!(
            s.starts_with("category-job-service-subs/33333333-4444-5555-6666-777777777777/image/")
        );
        assert!(s.ends_with(".webp"));
    }
}
