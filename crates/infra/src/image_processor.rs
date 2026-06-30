//! Image processor — decode arbitrary image bytes, transcode to
//! lossy WebP, and store via the `Storage` port (M9-extra / T-16).
//!
//! This is a **library function**, not an API endpoint. Callers
//! (admin handlers, seeders, CLI tools, future upload endpoints)
//! feed raw image bytes in and get back a `StorageKey` they can
//! persist in the database.
//!
//! ## Why WebP
//!
//! - 25-35% smaller than equivalent-quality JPEG (the most
//!   common upload format from mobile cameras).
//! - 26% smaller than PNG when the source has any photographic
//!   content.
//! - Lossy + lossless + alpha in one format — no second pipeline
//!   for transparent assets.
//!
//! ## Pipeline
//!
//! 1. **Size guard** — reject before decode (`max_input_bytes`)
//!    so a single malicious upload can't OOM the process.
//! 2. **Decode** — `image::load_from_memory` auto-sniffs the
//!    format (JPEG, PNG, GIF, BMP, WebP, ...). Anything it
//!    doesn't recognise bubbles up as `ImageError::Decode`.
//! 3. **Resize** — if the longest side exceeds
//!    `max_dimension_px`, downscale with Lanczos3 (the
//!    `image` crate's highest-quality filter).
//! 4. **Convert to RGB8** — strip alpha. WebP supports alpha
//!    but the savings are small for ID-card / portrait photos,
//!    and `RGB8` is the lowest-common-denominator colour type
//!    the `image-webp` encoder is happy with.
//! 5. **Encode** — lossy WebP at the configured quality (default 80).
//! 6. **Store** — hand the bytes to the `Storage` port. The key
//!    comes from [`kokkak_infra::storage::keys`] so the folder
//!    layout stays consistent across adapters.
//!
//! ponytail: the whole pipeline is in-memory. A 5 MiB JPEG
//! expands to ~50 MiB decoded and back down to a 200-500 KiB
//! WebP. Peak working set is therefore dominated by the decoded
//! image, not the input. For the current user-attachment use
//! case (≤ 5 MiB) this is comfortable on a 256 MiB container.
//! Upgrade path: `image::ImageDecoder` + `webp` encoder both
//! support streaming; switch when the cap rises above 50 MiB.

use std::sync::Arc;

use bytes::Bytes;
use image::imageops::FilterType;
use kokkak_domain::Storage;
use thiserror::Error;
use tracing::{debug, warn};

use crate::storage::keys;
// Re-export so callers don't need a second import path.
pub use crate::storage::keys::UserAttachment as UserAttachmentKind;

/// What kind of user image is being processed. Picks the folder
/// layout (see `storage::keys`) for the resulting `StorageKey`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserImageKind {
    /// Profile image (`user_img_profile_img_path`).
    Profile,
    /// Bank book cover (`user_bank_account_book_img_path`).
    BankBook,
    /// One of the four attachment kinds on `admin/users/full`.
    Attachment(UserAttachmentKind),
}

/// Image-processor configuration (subset of `ImageProcessorSettings`).
#[derive(Debug, Clone)]
pub struct ImageProcessorConfig {
    /// Cap on raw input bytes. Bigger → `ImageError::TooLarge`.
    pub max_input_bytes: usize,
    /// Longest-side cap after resize. `0` disables resize.
    pub max_dimension_px: u32,
    /// WebP lossy quality, 1..=100.
    pub webp_quality: u8,
}

/// What a successful `process_and_store` produced.
#[derive(Debug, Clone)]
pub struct ProcessedImage {
    /// Key under which the WebP bytes are stored. Feed this into
    /// `profile_img_path` / `bank_book_img_path` / `*_path` etc.
    pub key: kokkak_domain::StorageKey,
    /// Raw input size (before decode).
    pub original_size: usize,
    /// Encoded WebP size on disk.
    pub webp_size: usize,
    /// Decoded (post-resize) width in pixels.
    pub width: u32,
    /// Decoded (post-resize) height in pixels.
    pub height: u32,
    /// SHA-256 hex of the stored WebP bytes (returned by
    /// `Storage::put`).
    pub sha256: String,
}

/// Errors raised by the image processor.
#[derive(Debug, Error)]
pub enum ImageError {
    /// Input larger than `max_input_bytes` (rejected before decode).
    #[error("image too large: {0} bytes (max {1})")]
    TooLarge(usize, usize),
    /// Input bytes don't look like any image format the
    /// `image` crate knows about.
    #[error("image decode failed: {0}")]
    Decode(String),
    /// WebP encode failed (e.g. invalid quality knob).
    #[error("image encode failed: {0}")]
    Encode(String),
    /// Underlying `Storage` port raised an error.
    #[error("storage error: {0}")]
    Storage(#[from] kokkak_domain::StorageError),
    /// Caller passed an empty `user_guid` (would land under
    /// `users//profile/...` — useless).
    #[error("user_guid must not be empty")]
    EmptyUserGuid,
}

/// Reusable image-processing pipeline bound to a `Storage` port.
///
/// Construct once per process (the processor is cheap to share
/// — `Arc<dyn Storage>` is the only state). Call
/// [`process_and_store`](Self::process_and_store) per upload.
#[derive(Clone)]
pub struct ImageProcessor {
    storage: Arc<dyn Storage>,
    config: ImageProcessorConfig,
}

impl ImageProcessor {
    /// Build a new processor. The `Storage` port is held by
    /// `Arc` so cloning the processor is cheap.
    pub fn new(storage: Arc<dyn Storage>, config: ImageProcessorConfig) -> Self {
        Self { storage, config }
    }

    /// The bound `Storage` port (handy for tests + callers that
    /// need to chain additional storage calls after the
    /// transcode).
    pub fn storage(&self) -> Arc<dyn Storage> {
        Arc::clone(&self.storage)
    }

    /// Process raw image bytes → WebP → store.
    ///
    /// `bytes` may be JPEG / PNG / GIF / BMP / WebP / TIFF. The
    /// output is always **lossy WebP**, quality-controlled by
    /// [`ImageProcessorConfig::webp_quality`].
    pub async fn process_and_store(
        &self,
        bytes: &[u8],
        user_guid: &str,
        kind: UserImageKind,
    ) -> Result<ProcessedImage, ImageError> {
        if user_guid.is_empty() {
            return Err(ImageError::EmptyUserGuid);
        }
        if bytes.len() > self.config.max_input_bytes {
            return Err(ImageError::TooLarge(
                bytes.len(),
                self.config.max_input_bytes,
            ));
        }

        // 1. Decode. `load_from_memory` sniffs the format
        //    (magic bytes) and picks the matching decoder.
        let mut img =
            image::load_from_memory(bytes).map_err(|e| ImageError::Decode(e.to_string()))?;
        let original_w = img.width();
        let original_h = img.height();

        // 2. Resize if too large. Longest-side scaling
        //    preserves aspect ratio.
        if self.config.max_dimension_px > 0
            && (img.width() > self.config.max_dimension_px
                || img.height() > self.config.max_dimension_px)
        {
            img = img.resize(
                self.config.max_dimension_px,
                self.config.max_dimension_px,
                FilterType::Lanczos3,
            );
            debug!(
                original = format!("{original_w}x{original_h}"),
                resized = format!("{}x{}", img.width(), img.height()),
                "image downscaled"
            );
        }

        // 3. RGB8 — strip alpha. WebP *can* carry alpha but
        //    the saving is small for ID-card / portrait photos
        //    and the encoder path is simpler.
        let rgb = img.to_rgb8();
        let (w, h) = (rgb.width(), rgb.height());

        // 4. Encode lossy WebP via the `webp` crate. Quality
        //    is f32 in their API — clamp to a sane range so a
        //    misconfigured knob doesn't panic.
        let quality = self.config.webp_quality.clamp(1, 100) as f32;
        let webp_buf = webp::Encoder::from_rgb(rgb.as_raw(), rgb.width(), rgb.height())
            .encode(quality)
            .to_vec();

        // 5. Pick the key. The `.webp` extension is hard-coded
        //    — every output is WebP.
        let key = match kind {
            UserImageKind::Profile => keys::user_profile(user_guid, "webp"),
            UserImageKind::BankBook => keys::user_bank_book(user_guid, "webp"),
            UserImageKind::Attachment(a) => keys::user_attachment(user_guid, a, "webp"),
        };

        // 6. Store.
        let payload = Bytes::from(webp_buf.clone());
        let result = self.storage.put(&key, payload, None).await?;
        let webp_size = webp_buf.len();

        if webp_size >= bytes.len() {
            // WebP beat the original only on tiny synthetic
            // images (e.g. 1x1 transparent PNGs). The savings
            // are usually 60-80% on real photos. A warn keeps
            // the regression visible without failing the
            // request.
            warn!(
                original = bytes.len(),
                webp = webp_size,
                key = %key,
                "webp output not smaller than input"
            );
        }

        Ok(ProcessedImage {
            key,
            original_size: bytes.len(),
            webp_size,
            width: w,
            height: h,
            sha256: result.sha256,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStorage;
    use image::{ImageBuffer, Rgb};

    /// Build a tiny in-memory JPEG test image (no alpha, 8x8 red).
    fn tiny_jpeg() -> Vec<u8> {
        use std::io::Cursor;
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
            ImageBuffer::from_fn(8, 8, |_x, _y| Rgb([255, 0, 0]));
        let mut buf: Vec<u8> = Vec::new();
        let mut cursor = Cursor::new(&mut buf);
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, 90)
            .encode(
                img.as_raw(),
                img.width(),
                img.height(),
                image::ExtendedColorType::Rgb8,
            )
            .unwrap();
        buf
    }

    /// Build a tiny PNG with alpha (red, 8x8, all pixels
    /// fully opaque but the alpha channel is present).
    fn tiny_png_with_alpha() -> Vec<u8> {
        use std::io::Cursor;
        let img: ImageBuffer<image::Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_fn(8, 8, |_x, _y| image::Rgba([255, 0, 0, 255]));
        let mut buf: Vec<u8> = Vec::new();
        let mut cursor = Cursor::new(&mut buf);
        let encoder = image::codecs::png::PngEncoder::new(&mut cursor);
        image::ImageEncoder::write_image(
            encoder,
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
        buf
    }

    fn processor() -> ImageProcessor {
        ImageProcessor::new(
            Arc::new(MemoryStorage::new()),
            ImageProcessorConfig {
                max_input_bytes: 1024 * 1024,
                max_dimension_px: 16,
                webp_quality: 80,
            },
        )
    }

    #[tokio::test]
    async fn decodes_jpeg_and_stores_as_webp() {
        let p = processor();
        let r = p
            .process_and_store(&tiny_jpeg(), "user-1", UserImageKind::Profile)
            .await
            .unwrap();
        assert_eq!(r.width, 8);
        assert_eq!(r.height, 8);
        assert!(r.key.as_str().starts_with("users/user-1/profile/"));
        assert!(r.key.as_str().ends_with(".webp"));
        // WebP file header is `RIFF????WEBP`.
        let blob = p.storage.get(&r.key).await.unwrap().unwrap();
        assert_eq!(&blob[0..4], b"RIFF");
        assert_eq!(&blob[8..12], b"WEBP");
        // Sanity: original JPEG is bigger than the WebP output
        // even at quality 80 (8x8 red is degenerate, but
        // acceptable here).
        assert!(r.webp_size < r.original_size);
    }

    #[tokio::test]
    async fn decodes_png_with_alpha() {
        let p = processor();
        let r = p
            .process_and_store(
                &tiny_png_with_alpha(),
                "user-2",
                UserImageKind::Attachment(UserAttachmentKind::IdCardFront),
            )
            .await
            .unwrap();
        assert!(r.key.as_str().contains("attachments/id-card-front/"));
        assert!(r.key.as_str().ends_with(".webp"));
    }

    #[tokio::test]
    async fn resizes_when_above_max_dimension() {
        use std::io::Cursor;
        // 64x64 input, max_dim=16 → output should be 16x16.
        let big: ImageBuffer<Rgb<u8>, Vec<u8>> =
            ImageBuffer::from_fn(64, 64, |_x, _y| Rgb([0, 128, 0]));
        let mut buf: Vec<u8> = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut Cursor::new(&mut buf), 90)
            .encode(
                big.as_raw(),
                big.width(),
                big.height(),
                image::ExtendedColorType::Rgb8,
            )
            .unwrap();

        let p = ImageProcessor::new(
            Arc::new(MemoryStorage::new()),
            ImageProcessorConfig {
                max_input_bytes: 1024 * 1024,
                max_dimension_px: 16,
                webp_quality: 80,
            },
        );
        let r = p
            .process_and_store(&buf, "user-3", UserImageKind::Profile)
            .await
            .unwrap();
        assert_eq!(r.width, 16);
        assert_eq!(r.height, 16);
    }

    #[tokio::test]
    async fn rejects_too_large_input() {
        let p = ImageProcessor::new(
            Arc::new(MemoryStorage::new()),
            ImageProcessorConfig {
                max_input_bytes: 10, // tiny
                max_dimension_px: 16,
                webp_quality: 80,
            },
        );
        let r = p
            .process_and_store(&[0_u8; 100], "user-x", UserImageKind::Profile)
            .await;
        assert!(matches!(r, Err(ImageError::TooLarge(100, 10))));
    }

    #[tokio::test]
    async fn rejects_empty_user_guid() {
        let p = processor();
        let r = p
            .process_and_store(&tiny_jpeg(), "", UserImageKind::Profile)
            .await;
        assert!(matches!(r, Err(ImageError::EmptyUserGuid)));
    }

    #[tokio::test]
    async fn rejects_invalid_image_bytes() {
        let p = processor();
        // 0xFF 0x00 0x00 is the JPEG SOI + a broken segment,
        // enough to fail format detection for sure; use plain
        // text to be unambiguous.
        let r = p
            .process_and_store(b"not an image", "user-z", UserImageKind::Profile)
            .await;
        assert!(matches!(r, Err(ImageError::Decode(_))));
    }

    #[tokio::test]
    async fn bank_book_key_uses_bank_book_folder() {
        let p = processor();
        let r = p
            .process_and_store(&tiny_jpeg(), "user-9", UserImageKind::BankBook)
            .await
            .unwrap();
        assert!(r.key.as_str().contains("/bank-book/"));
    }
}
