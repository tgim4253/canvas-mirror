use std::{fs, io::Cursor, path::Path};

use image::{imageops::FilterType, DynamicImage, GenericImageView, ImageFormat};
use image_server_store::OutputResolution;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PreviewError {
    #[error("preview generation task failed: {0}")]
    GenerateTask(#[from] tokio::task::JoinError),
    #[error("clip preview extraction failed: {0}")]
    ClipPreview(#[from] clip2preview::ClipError),
    #[error("failed to read preview source: {0}")]
    ReadSource(#[from] std::io::Error),
    #[error("failed to decode or resize preview: {0}")]
    Image(#[from] image::ImageError),
    #[error("unsupported preview input: {path}")]
    UnsupportedInput { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedPreview {
    pub bytes: Vec<u8>,
    pub mime_type: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LoadedPreview {
    bytes: Vec<u8>,
    mime_type: String,
    dimensions: Option<(u32, u32)>,
}

#[derive(Debug, Default, Clone)]
pub struct PreviewGenerator;

impl PreviewGenerator {
    pub async fn generate(
        &self,
        input: &Path,
        resolution: &OutputResolution,
    ) -> Result<GeneratedPreview, PreviewError> {
        let input = input.to_path_buf();
        let resolution = resolution.clone();

        tokio::task::spawn_blocking(move || generate_preview_from_disk(&input, &resolution))
            .await
            .map_err(PreviewError::GenerateTask)?
    }
}

fn generate_preview_from_disk(
    input: &Path,
    resolution: &OutputResolution,
) -> Result<GeneratedPreview, PreviewError> {
    let preview = match input_kind(input)? {
        InputKind::Clip => load_clip_preview(input)?,
        InputKind::StaticImage => load_static_image_preview(input)?,
    };

    apply_resolution(preview, resolution)
}

fn apply_resolution(
    preview: LoadedPreview,
    resolution: &OutputResolution,
) -> Result<GeneratedPreview, PreviewError> {
    let original_bytes = preview.bytes;
    let original_mime = preview.mime_type;
    let original_dimensions = preview.dimensions;

    match resolution {
        OutputResolution::Source => Ok(GeneratedPreview {
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
                return Ok(GeneratedPreview {
                    bytes: original_bytes,
                    mime_type: original_mime,
                    width: None,
                    height: None,
                });
            };

            if width <= *max_width && height <= *max_height {
                return Ok(GeneratedPreview {
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

            Ok(GeneratedPreview {
                bytes: encoded_bytes,
                mime_type: encoded_mime,
                width: Some(resized_width),
                height: Some(resized_height),
            })
        }
    }
}

fn load_clip_preview(input: &Path) -> Result<LoadedPreview, PreviewError> {
    let preview = clip2preview::extract_preview(input)?;

    Ok(LoadedPreview {
        bytes: preview.bytes().to_vec(),
        mime_type: preview.format().media_type().to_string(),
        dimensions: preview.dimensions(),
    })
}

fn load_static_image_preview(input: &Path) -> Result<LoadedPreview, PreviewError> {
    let bytes = fs::read(input)?;
    let image = image::load_from_memory(&bytes)?;
    let format = image::guess_format(&bytes).ok();

    Ok(LoadedPreview {
        bytes,
        mime_type: format
            .and_then(image_format_to_mime)
            .or_else(|| extension_to_mime(input))
            .unwrap_or("image/png")
            .to_string(),
        dimensions: Some(image.dimensions()),
    })
}

fn encode_image(
    image: &DynamicImage,
    original_mime: &str,
) -> Result<(Vec<u8>, String), PreviewError> {
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

fn write_image(image: &DynamicImage, format: ImageFormat) -> Result<Vec<u8>, PreviewError> {
    let mut cursor = Cursor::new(Vec::new());
    image.write_to(&mut cursor, format)?;
    Ok(cursor.into_inner())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputKind {
    Clip,
    StaticImage,
}

fn input_kind(input: &Path) -> Result<InputKind, PreviewError> {
    match input
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("clip") => Ok(InputKind::Clip),
        Some("png" | "jpg" | "jpeg" | "webp") => Ok(InputKind::StaticImage),
        _ => Err(PreviewError::UnsupportedInput {
            path: input.display().to_string(),
        }),
    }
}

fn extension_to_mime(input: &Path) -> Option<&'static str> {
    match input
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => Some("image/png"),
        Some("jpg" | "jpeg") => Some("image/jpeg"),
        Some("webp") => Some("image/webp"),
        _ => None,
    }
}

fn image_format_to_mime(format: ImageFormat) -> Option<&'static str> {
    match format {
        ImageFormat::Png => Some("image/png"),
        ImageFormat::Jpeg => Some("image/jpeg"),
        ImageFormat::WebP => Some("image/webp"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[tokio::test]
    async fn generates_static_png_source_preview() {
        let dir = tempfile::tempdir().expect("temp dir should exist");
        let image_path = dir.path().join("source.png");
        write_sample_png(&image_path, 96, 64);

        let preview = PreviewGenerator
            .generate(&image_path, &OutputResolution::Source)
            .await
            .expect("png preview should generate");

        assert_eq!(preview.mime_type, "image/png");
        assert_eq!(preview.width, Some(96));
        assert_eq!(preview.height, Some(64));
        assert_eq!(
            preview.bytes,
            fs::read(&image_path).expect("png bytes should read")
        );
    }

    #[tokio::test]
    async fn resizes_static_png_for_contain_resolution() {
        let dir = tempfile::tempdir().expect("temp dir should exist");
        let image_path = dir.path().join("resized.png");
        write_sample_png(&image_path, 200, 100);

        let preview = PreviewGenerator
            .generate(
                &image_path,
                &OutputResolution::Contain {
                    max_width: 50,
                    max_height: 50,
                },
            )
            .await
            .expect("png preview should resize");

        assert_eq!(preview.mime_type, "image/png");
        assert_eq!(preview.width, Some(50));
        assert_eq!(preview.height, Some(25));
    }

    #[tokio::test]
    async fn rejects_unsupported_input_extension() {
        let dir = tempfile::tempdir().expect("temp dir should exist");
        let text_path = dir.path().join("note.txt");
        fs::write(&text_path, "not an image").expect("text file should write");

        let error = PreviewGenerator
            .generate(&text_path, &OutputResolution::Source)
            .await
            .expect_err("unsupported file should fail");

        assert!(matches!(error, PreviewError::UnsupportedInput { .. }));
    }

    fn write_sample_png(path: &Path, width: u32, height: u32) {
        let image = DynamicImage::new_rgba8(width, height);
        image.save(path).expect("sample png should save");
    }
}
