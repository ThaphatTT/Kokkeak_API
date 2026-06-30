//! `StorageKey` builders for the user-attachments flows (M9-extra).
//!
//! Every builder returns a relative key like
//! `users/{user_guid}/profile/{uuid}.{ext}`. The `LocalStorage`
//! adapter (and any future adapter) treats this as a path under
//! the configured root, so the folder layout stays consistent
//! across adapters and human-inspectable on disk.
//!
//! ponytail: the UUID v7 makes the filename time-sortable + unique
//! without forcing the caller to thread a counter. The extension
//! is preserved as-supplied (case-insensitive); the adapter does
//! not validate it — that's the caller's job (the upload endpoint
//! would sniff the magic bytes; this helper just builds the key).

use kokkak_domain::StorageKey;
use uuid::Uuid;

/// Build the `StorageKey` for a user's primary profile image.
///
/// Layout: `users/{user_guid}/profile/{uuid}.{ext}`
pub fn user_profile(user_guid: &str, ext: &str) -> StorageKey {
    StorageKey(format!(
        "users/{}/profile/{}.{}",
        user_guid,
        Uuid::now_v7(),
        normalize_ext(ext)
    ))
}

/// Build the `StorageKey` for a user's `bank_book_img_path`.
///
/// Layout: `users/{user_guid}/bank-book/{uuid}.{ext}`
pub fn user_bank_book(user_guid: &str, ext: &str) -> StorageKey {
    StorageKey(format!(
        "users/{}/bank-book/{}.{}",
        user_guid,
        Uuid::now_v7(),
        normalize_ext(ext)
    ))
}

/// Attachment kinds for the four `*_path` fields on
/// `admin/users/full` (id-card front / back, proof of address,
/// source-of-funds statement).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserAttachment {
    /// `id_card_front_path`.
    IdCardFront,
    /// `id_card_back_path`.
    IdCardBack,
    /// `proof_of_address_path`.
    ProofOfAddress,
    /// `source_of_funds_statement_path`.
    SourceOfFunds,
}

impl UserAttachment {
    /// Folder segment for this kind (used in the key path).
    fn folder(self) -> &'static str {
        match self {
            UserAttachment::IdCardFront => "id-card-front",
            UserAttachment::IdCardBack => "id-card-back",
            UserAttachment::ProofOfAddress => "proof-of-address",
            UserAttachment::SourceOfFunds => "source-of-funds",
        }
    }
}

/// Build the `StorageKey` for a user attachment.
///
/// Layout: `users/{user_guid}/attachments/{kind}/{uuid}.{ext}`
pub fn user_attachment(user_guid: &str, kind: UserAttachment, ext: &str) -> StorageKey {
    StorageKey(format!(
        "users/{}/attachments/{}/{}.{}",
        user_guid,
        kind.folder(),
        Uuid::now_v7(),
        normalize_ext(ext)
    ))
}

/// Lowercase + strip a leading `.` so callers can pass either
/// `jpg` or `.jpg`. Empty / unknown stays empty (caller's job
/// to validate the upload MIME type).
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
            // Each kind lands in its own folder.
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
}
