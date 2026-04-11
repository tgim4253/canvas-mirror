use std::{io::Cursor, path::Path, sync::Arc};

use async_trait::async_trait;
use image::{imageops::FilterType, DynamicImage, GenericImageView, ImageFormat};
use image_server_store::OutputResolution;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExtractorError {
    #[error("preview extraction task failed: {0}")]
    ExtractTask(#[from] tokio::task::JoinError),
    #[error("clip preview extraction failed: {0}")]
    ExtractPreview(#[from] clip2preview::ClipError),
    #[error("failed to resize preview: {0}")]
    Resize(#[from] image::ImageError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedPreview {
    pub bytes: Vec<u8>,
    pub mime_type: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[async_trait]
pub trait PreviewExtractor: Send + Sync {
    async fn extract(
        &self,
        input: &Path,
        resolution: &OutputResolution,
    ) -> Result<ExtractedPreview, ExtractorError>;
}

#[derive(Debug, Default, Clone)]
pub struct ClipPreviewExtractor;

#[async_trait]
impl PreviewExtractor for ClipPreviewExtractor {
    async fn extract(
        &self,
        input: &Path,
        resolution: &OutputResolution,
    ) -> Result<ExtractedPreview, ExtractorError> {
        let input = input.to_path_buf();
        let resolution = resolution.clone();

        tokio::task::spawn_blocking(move || extract_preview_from_disk(&input, &resolution))
            .await
            .map_err(ExtractorError::ExtractTask)?
    }
}

#[derive(Debug, Clone)]
pub struct MockPreviewExtractor {
    preview: Arc<ExtractedPreview>,
}

impl MockPreviewExtractor {
    pub fn new(preview: ExtractedPreview) -> Self {
        Self {
            preview: Arc::new(preview),
        }
    }
}

#[async_trait]
impl PreviewExtractor for MockPreviewExtractor {
    async fn extract(
        &self,
        _input: &Path,
        _resolution: &OutputResolution,
    ) -> Result<ExtractedPreview, ExtractorError> {
        Ok((*self.preview).clone())
    }
}

fn extract_preview_from_disk(
    input: &Path,
    resolution: &OutputResolution,
) -> Result<ExtractedPreview, ExtractorError> {
    let preview = clip2preview::extract_preview(input)?;
    let original_bytes = preview.bytes().to_vec();
    let original_mime = preview.format().media_type().to_string();
    let original_dimensions = preview.dimensions();

    match resolution {
        OutputResolution::Source => Ok(ExtractedPreview {
            bytes: original_bytes,
            mime_type: original_mime,
            width: original_dimensions.map(|(width, _)| width),
            height: original_dimensions.map(|(_, height)| height),
        }),
        OutputResolution::Contain {
            max_width,
            max_height,
        } => {
            let Some((width, height)) = original_dimensions else {
                return Ok(ExtractedPreview {
                    bytes: original_bytes,
                    mime_type: original_mime,
                    width: None,
                    height: None,
                });
            };

            if width <= *max_width && height <= *max_height {
                return Ok(ExtractedPreview {
                    bytes: original_bytes,
                    mime_type: original_mime,
                    width: Some(width),
                    height: Some(height),
                });
            }

            let image = image::load_from_memory(&original_bytes)?;
            let resized = image.resize(*max_width, *max_height, FilterType::Lanczos3);
            let (resized_width, resized_height) = resized.dimensions();
            let (encoded_bytes, encoded_mime) = encode_image(&resized, &original_mime)?;

            Ok(ExtractedPreview {
                bytes: encoded_bytes,
                mime_type: encoded_mime,
                width: Some(resized_width),
                height: Some(resized_height),
            })
        }
    }
}

fn encode_image(
    image: &DynamicImage,
    original_mime: &str,
) -> Result<(Vec<u8>, String), ExtractorError> {
    let preferred = match original_mime {
        "image/png" => Some((ImageFormat::Png, "image/png")),
        "image/jpeg" => Some((ImageFormat::Jpeg, "image/jpeg")),
        "image/webp" => Some((ImageFormat::WebP, "image/webp")),
        _ => None,
    };

    if let Some((format, mime_type)) = preferred {
        if let Ok(bytes) = write_image(image, format) {
            return Ok((bytes, mime_type.to_string()));
        }
    }

    Ok((
        write_image(image, ImageFormat::Png)?,
        "image/png".to_string(),
    ))
}

fn write_image(image: &DynamicImage, format: ImageFormat) -> Result<Vec<u8>, ExtractorError> {
    let mut cursor = Cursor::new(Vec::new());
    image.write_to(&mut cursor, format)?;
    Ok(cursor.into_inner())
}
