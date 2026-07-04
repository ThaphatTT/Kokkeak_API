

use std::sync::Arc;

use bytes::Bytes;
use image::imageops::FilterType;
use kokkak_domain::Storage;
use thiserror::Error;
use tracing::{debug, warn};

use crate::storage::keys;

pub use crate::storage::keys::UserAttachment as UserAttachmentKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserImageKind {

    Profile,

    BankBook,

    Attachment(UserAttachmentKind),
}

#[derive(Debug, Clone)]
pub struct ImageProcessorConfig {

    pub max_input_bytes: usize,

    pub max_dimension_px: u32,

    pub webp_quality: u8,
}

#[derive(Debug, Clone)]
pub struct ProcessedImage {

    pub key: kokkak_domain::StorageKey,

    pub original_size: usize,

    pub webp_size: usize,

    pub width: u32,

    pub height: u32,

    pub sha256: String,
}

#[derive(Debug, Error)]
pub enum ImageError {

    #[error("image too large: {0} bytes (max {1})")]
    TooLarge(usize, usize),

    #[error("image decode failed: {0}")]
    Decode(String),

    #[error("image encode failed: {0}")]
    Encode(String),

    #[error("storage error: {0}")]
    Storage(#[from] kokkak_domain::StorageError),

    #[error("user_guid must not be empty")]
    EmptyUserGuid,
}

#[derive(Clone)]
pub struct ImageProcessor {
    storage: Arc<dyn Storage>,
    config: ImageProcessorConfig,
}

impl ImageProcessor {

    pub fn new(storage: Arc<dyn Storage>, config: ImageProcessorConfig) -> Self {
        Self { storage, config }
    }

    pub fn storage(&self) -> Arc<dyn Storage> {
        Arc::clone(&self.storage)
    }

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

        let mut img =
            image::load_from_memory(bytes).map_err(|e| ImageError::Decode(e.to_string()))?;
        let original_w = img.width();
        let original_h = img.height();

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

        let rgb = img.to_rgb8();
        let (w, h) = (rgb.width(), rgb.height());

        let quality = self.config.webp_quality.clamp(1, 100) as f32;
        let webp_buf = webp::Encoder::from_rgb(rgb.as_raw(), rgb.width(), rgb.height())
            .encode(quality)
            .to_vec();

        let key = match kind {
            UserImageKind::Profile => keys::user_profile(user_guid, "webp"),
            UserImageKind::BankBook => keys::user_bank_book(user_guid, "webp"),
            UserImageKind::Attachment(a) => keys::user_attachment(user_guid, a, "webp"),
        };

        let payload = Bytes::from(webp_buf.clone());
        let result = self.storage.put(&key, payload, None).await?;
        let webp_size = webp_buf.len();

        if webp_size >= bytes.len() {

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

        let blob = p.storage.get(&r.key).await.unwrap().unwrap();
        assert_eq!(&blob[0..4], b"RIFF");
        assert_eq!(&blob[8..12], b"WEBP");

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
                max_input_bytes: 10,
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
