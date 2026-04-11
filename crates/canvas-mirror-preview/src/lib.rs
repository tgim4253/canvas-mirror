use std::{fs, io::Cursor, path::Path};

use canvas_mirror_store::OutputResolution;
use image::{imageops::FilterType, DynamicImage, GenericImageView, ImageFormat};
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
    #[error("failed to extract PSD preview from {path}: {reason}")]
    PsdPreview { path: String, reason: String },
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
        InputKind::Psd => load_psd_preview(input)?,
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

fn load_psd_preview(input: &Path) -> Result<LoadedPreview, PreviewError> {
    let bytes = fs::read(input)?;

    extract_psd_thumbnail(&bytes).map_err(|reason| PreviewError::PsdPreview {
        path: input.display().to_string(),
        reason,
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
    Psd,
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
        Some("psd") => Ok(InputKind::Psd),
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

const PSD_HEADER_LEN: usize = 26;
const PSD_THUMBNAIL_HEADER_LEN: usize = 28;
const PSD_THUMBNAIL_RESOURCE_IDS: [u16; 2] = [1036, 1033];

fn extract_psd_thumbnail(bytes: &[u8]) -> Result<LoadedPreview, String> {
    if slice_range(bytes, 0, 4, bytes.len(), "PSD signature")? != b"8BPS" {
        return Err("missing PSD signature".to_string());
    }

    let version = read_be_u16(bytes, 4, bytes.len(), "PSD version")?;
    if version != 1 {
        return Err(format!("unsupported PSD version {version}"));
    }

    let mut offset = PSD_HEADER_LEN;
    let color_mode_len =
        read_be_u32(bytes, offset, bytes.len(), "PSD color mode data length")? as usize;
    offset += 4;
    offset = checked_advance(offset, color_mode_len, bytes.len(), "PSD color mode data")?;

    let resources_len =
        read_be_u32(bytes, offset, bytes.len(), "PSD image resources length")? as usize;
    offset += 4;
    let resources_end = checked_advance(
        offset,
        resources_len,
        bytes.len(),
        "PSD image resources section",
    )?;

    let mut resource_offset = offset;
    let mut thumbnail_error = None;

    while resource_offset < resources_end {
        if slice_range(
            bytes,
            resource_offset,
            4,
            resources_end,
            "PSD image resource signature",
        )? != b"8BIM"
        {
            return Err(format!(
                "invalid PSD image resource signature at byte offset {resource_offset}"
            ));
        }
        resource_offset += 4;

        let resource_id = read_be_u16(
            bytes,
            resource_offset,
            resources_end,
            "PSD image resource id",
        )?;
        resource_offset += 2;

        let name_len = read_u8(
            bytes,
            resource_offset,
            resources_end,
            "PSD image resource name length",
        )? as usize;
        resource_offset += 1;
        resource_offset = checked_advance(
            resource_offset,
            name_len,
            resources_end,
            "PSD image resource name",
        )?;
        if (1 + name_len) % 2 != 0 {
            resource_offset = checked_advance(
                resource_offset,
                1,
                resources_end,
                "PSD image resource name padding",
            )?;
        }

        let payload_len = read_be_u32(
            bytes,
            resource_offset,
            resources_end,
            "PSD image resource payload length",
        )? as usize;
        resource_offset += 4;

        let payload_end = checked_advance(
            resource_offset,
            payload_len,
            resources_end,
            "PSD image resource payload",
        )?;
        let payload = &bytes[resource_offset..payload_end];

        if PSD_THUMBNAIL_RESOURCE_IDS.contains(&resource_id) {
            match parse_psd_thumbnail_payload(payload) {
                Ok(preview) => return Ok(preview),
                Err(reason) => thumbnail_error = Some(reason),
            }
        }

        resource_offset = payload_end;
        if payload_len % 2 != 0 {
            resource_offset = checked_advance(
                resource_offset,
                1,
                resources_end,
                "PSD image resource payload padding",
            )?;
        }
    }

    if let Some(reason) = thumbnail_error {
        return Err(reason);
    }

    Err("missing PSD thumbnail resource 1036/1033".to_string())
}

fn parse_psd_thumbnail_payload(payload: &[u8]) -> Result<LoadedPreview, String> {
    let format = read_be_u32(payload, 0, payload.len(), "PSD thumbnail format")?;
    let width = read_be_u32(payload, 4, payload.len(), "PSD thumbnail width")?;
    let height = read_be_u32(payload, 8, payload.len(), "PSD thumbnail height")?;
    let _widthbytes = read_be_u32(payload, 12, payload.len(), "PSD thumbnail widthbytes")?;
    let _total_size = read_be_u32(payload, 16, payload.len(), "PSD thumbnail total size")?;
    let compressed_size =
        read_be_u32(payload, 20, payload.len(), "PSD thumbnail compressed size")? as usize;
    let _bits_per_pixel = read_be_u16(payload, 24, payload.len(), "PSD thumbnail bits per pixel")?;
    let _planes = read_be_u16(payload, 26, payload.len(), "PSD thumbnail planes")?;

    if format != 1 {
        return Err(format!(
            "unsupported PSD thumbnail format {format}, expected JPEG (1)"
        ));
    }

    let jpeg_bytes = slice_range(
        payload,
        PSD_THUMBNAIL_HEADER_LEN,
        compressed_size,
        payload.len(),
        "PSD thumbnail JPEG bytes",
    )?
    .to_vec();

    Ok(LoadedPreview {
        bytes: jpeg_bytes,
        mime_type: "image/jpeg".to_string(),
        dimensions: Some((width, height)),
    })
}

fn read_u8(bytes: &[u8], offset: usize, limit: usize, label: &str) -> Result<u8, String> {
    Ok(slice_range(bytes, offset, 1, limit, label)?[0])
}

fn read_be_u16(bytes: &[u8], offset: usize, limit: usize, label: &str) -> Result<u16, String> {
    let slice = slice_range(bytes, offset, 2, limit, label)?;
    Ok(u16::from_be_bytes([slice[0], slice[1]]))
}

fn read_be_u32(bytes: &[u8], offset: usize, limit: usize, label: &str) -> Result<u32, String> {
    let slice = slice_range(bytes, offset, 4, limit, label)?;
    Ok(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn slice_range<'a>(
    bytes: &'a [u8],
    offset: usize,
    len: usize,
    limit: usize,
    label: &str,
) -> Result<&'a [u8], String> {
    let end = checked_advance(offset, len, limit, label)?;
    Ok(&bytes[offset..end])
}

fn checked_advance(offset: usize, len: usize, limit: usize, label: &str) -> Result<usize, String> {
    let end = offset
        .checked_add(len)
        .ok_or_else(|| format!("{label} offset overflow"))?;

    if end > limit {
        return Err(format!("{label} exceeds section bounds"));
    }

    Ok(end)
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
    async fn extracts_psd_embedded_jpeg_preview_from_resource_1036() {
        let dir = tempfile::tempdir().expect("temp dir should exist");
        let image_path = dir.path().join("sample.psd");
        let expected_bytes = write_sample_psd(&image_path, 160, 101, 1036);

        let preview = PreviewGenerator
            .generate(&image_path, &OutputResolution::Source)
            .await
            .expect("psd preview should extract");

        assert_eq!(preview.mime_type, "image/jpeg");
        assert_eq!(preview.width, Some(160));
        assert_eq!(preview.height, Some(101));
        assert_eq!(preview.bytes, expected_bytes);
    }

    #[tokio::test]
    async fn resizes_psd_embedded_jpeg_preview_from_resource_1033() {
        let dir = tempfile::tempdir().expect("temp dir should exist");
        let image_path = dir.path().join("legacy.psd");
        write_sample_psd(&image_path, 200, 100, 1033);

        let preview = PreviewGenerator
            .generate(
                &image_path,
                &OutputResolution::Contain {
                    max_width: 50,
                    max_height: 50,
                },
            )
            .await
            .expect("psd preview should resize");

        assert_eq!(preview.mime_type, "image/jpeg");
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

    fn write_sample_psd(path: &Path, width: u32, height: u32, resource_id: u16) -> Vec<u8> {
        let jpeg_bytes = write_image(&DynamicImage::new_rgb8(width, height), ImageFormat::Jpeg)
            .expect("sample jpeg should encode");
        let widthbytes = ((width * 24 + 31) / 32) * 4;
        let total_size = widthbytes * height;

        let mut thumbnail_payload = Vec::new();
        thumbnail_payload.extend_from_slice(&1_u32.to_be_bytes());
        thumbnail_payload.extend_from_slice(&width.to_be_bytes());
        thumbnail_payload.extend_from_slice(&height.to_be_bytes());
        thumbnail_payload.extend_from_slice(&widthbytes.to_be_bytes());
        thumbnail_payload.extend_from_slice(&total_size.to_be_bytes());
        thumbnail_payload.extend_from_slice(&(jpeg_bytes.len() as u32).to_be_bytes());
        thumbnail_payload.extend_from_slice(&24_u16.to_be_bytes());
        thumbnail_payload.extend_from_slice(&1_u16.to_be_bytes());
        thumbnail_payload.extend_from_slice(&jpeg_bytes);

        let mut resource_block = Vec::new();
        resource_block.extend_from_slice(b"8BIM");
        resource_block.extend_from_slice(&resource_id.to_be_bytes());
        resource_block.extend_from_slice(&[0, 0]);
        resource_block.extend_from_slice(&(thumbnail_payload.len() as u32).to_be_bytes());
        resource_block.extend_from_slice(&thumbnail_payload);
        if thumbnail_payload.len() % 2 != 0 {
            resource_block.push(0);
        }

        let mut psd_bytes = Vec::new();
        psd_bytes.extend_from_slice(b"8BPS");
        psd_bytes.extend_from_slice(&1_u16.to_be_bytes());
        psd_bytes.extend_from_slice(&[0; 6]);
        psd_bytes.extend_from_slice(&3_u16.to_be_bytes());
        psd_bytes.extend_from_slice(&height.to_be_bytes());
        psd_bytes.extend_from_slice(&width.to_be_bytes());
        psd_bytes.extend_from_slice(&8_u16.to_be_bytes());
        psd_bytes.extend_from_slice(&3_u16.to_be_bytes());
        psd_bytes.extend_from_slice(&0_u32.to_be_bytes());
        psd_bytes.extend_from_slice(&(resource_block.len() as u32).to_be_bytes());
        psd_bytes.extend_from_slice(&resource_block);

        fs::write(path, &psd_bytes).expect("sample psd should write");

        jpeg_bytes
    }
}
