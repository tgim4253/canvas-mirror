use std::{fs, io::Cursor, path::Path};

use canvas_mirror_store::{OutputResolution, StoredIccProfile};
use crc32fast::Hasher;
use flate2::{write::ZlibEncoder, Compression};
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
        icc_profile: Option<&StoredIccProfile>,
    ) -> Result<GeneratedPreview, PreviewError> {
        let input = input.to_path_buf();
        let resolution = resolution.clone();
        let icc_profile = icc_profile.cloned();

        tokio::task::spawn_blocking(move || {
            generate_preview_from_disk(&input, &resolution, icc_profile.as_ref())
        })
        .await
        .map_err(PreviewError::GenerateTask)?
    }
}

fn generate_preview_from_disk(
    input: &Path,
    resolution: &OutputResolution,
    icc_profile: Option<&StoredIccProfile>,
) -> Result<GeneratedPreview, PreviewError> {
    let preview = match input_kind(input)? {
        InputKind::Clip => load_clip_preview(input)?,
        InputKind::StaticImage => load_static_image_preview(input)?,
        InputKind::Psd => load_psd_preview(input)?,
    };

    let preview = apply_resolution(preview, resolution)?;
    Ok(apply_generated_preview_icc(preview, icc_profile))
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

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const PNG_IHDR_CHUNK_TYPE: [u8; 4] = *b"IHDR";
const PNG_ICCP_CHUNK_TYPE: [u8; 4] = *b"iCCP";
const PNG_COLOR_SPACE_CHUNK_TYPES: [[u8; 4]; 5] =
    [*b"sRGB", *b"iCCP", *b"gAMA", *b"cHRM", *b"cICP"];
const DEFAULT_PNG_ICC_PROFILE_NAME: &str = "Canvas Mirror ICC";
const PNG_ICC_COMPRESSION_METHOD: u8 = 0;

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

fn apply_generated_preview_icc(
    mut preview: GeneratedPreview,
    icc_profile: Option<&StoredIccProfile>,
) -> GeneratedPreview {
    let Some(icc_profile) = icc_profile else {
        return preview;
    };

    preview.bytes = apply_icc_profile_to_preview_bytes(
        preview.bytes,
        &preview.mime_type,
        preview.width.zip(preview.height),
        icc_profile,
    );
    preview
}

fn apply_icc_profile_to_preview_bytes(
    bytes: Vec<u8>,
    mime_type: &str,
    dimensions: Option<(u32, u32)>,
    icc_profile: &StoredIccProfile,
) -> Vec<u8> {
    match mime_type {
        "image/png" => replace_png_icc_chunk(bytes, icc_profile),
        "image/jpeg" => replace_jpeg_icc_profile(bytes, icc_profile),
        "image/webp" => replace_webp_icc_profile(bytes, dimensions, icc_profile),
        _ => bytes,
    }
}

fn replace_png_icc_chunk(bytes: Vec<u8>, icc_profile: &StoredIccProfile) -> Vec<u8> {
    let Some(insert_offset) = png_ihdr_end_offset(&bytes) else {
        return bytes;
    };

    let Some(stripped) = strip_png_color_space_chunks(&bytes) else {
        return bytes;
    };
    let profile_name = sanitize_png_icc_profile_name(&icc_profile.name);
    let compressed_profile = compress_png_icc_profile(&icc_profile.bytes);

    let mut chunk_data = Vec::with_capacity(profile_name.len() + compressed_profile.len() + 2);
    chunk_data.extend_from_slice(profile_name.as_bytes());
    chunk_data.push(0);
    chunk_data.push(PNG_ICC_COMPRESSION_METHOD);
    chunk_data.extend_from_slice(&compressed_profile);

    let chunk = build_png_chunk(PNG_ICCP_CHUNK_TYPE, &chunk_data);
    let mut normalized = Vec::with_capacity(stripped.len() + chunk.len());
    normalized.extend_from_slice(&stripped[..insert_offset]);
    normalized.extend_from_slice(&chunk);
    normalized.extend_from_slice(&stripped[insert_offset..]);
    normalized
}

fn sanitize_png_icc_profile_name(name: &str) -> String {
    let mut sanitized = name
        .chars()
        .filter_map(|character| match character {
            '\0' => None,
            character if character.is_ascii() => Some(character),
            _ => Some('?'),
        })
        .collect::<String>()
        .trim()
        .to_string();

    if sanitized.is_empty() {
        sanitized = DEFAULT_PNG_ICC_PROFILE_NAME.to_string();
    }

    while sanitized.len() > 79 {
        sanitized.pop();
    }

    sanitized
}

fn compress_png_icc_profile(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    if std::io::Write::write_all(&mut encoder, bytes).is_err() {
        return bytes.to_vec();
    }

    encoder.finish().unwrap_or_else(|_| bytes.to_vec())
}

fn png_ihdr_end_offset(bytes: &[u8]) -> Option<usize> {
    if !bytes.starts_with(PNG_SIGNATURE) {
        return None;
    }

    let length = read_be_u32(
        bytes,
        PNG_SIGNATURE.len(),
        bytes.len(),
        "PNG IHDR chunk length",
    )
    .ok()? as usize;
    let chunk_type = slice_range(
        bytes,
        PNG_SIGNATURE.len() + 4,
        4,
        bytes.len(),
        "PNG IHDR chunk type",
    )
    .ok()?;

    if chunk_type != PNG_IHDR_CHUNK_TYPE {
        return None;
    }

    let data_end = checked_advance(
        PNG_SIGNATURE.len() + 8,
        length,
        bytes.len(),
        "PNG IHDR chunk data",
    )
    .ok()?;

    checked_advance(data_end, 4, bytes.len(), "PNG IHDR chunk crc").ok()
}

fn strip_png_color_space_chunks(bytes: &[u8]) -> Option<Vec<u8>> {
    if !bytes.starts_with(PNG_SIGNATURE) {
        return None;
    }

    let mut normalized = Vec::with_capacity(bytes.len());
    normalized.extend_from_slice(PNG_SIGNATURE);
    let mut offset = PNG_SIGNATURE.len();

    while offset < bytes.len() {
        let length = read_be_u32(bytes, offset, bytes.len(), "PNG chunk length").ok()? as usize;
        let chunk_type_offset = offset + 4;
        let chunk_type =
            slice_range(bytes, chunk_type_offset, 4, bytes.len(), "PNG chunk type").ok()?;
        let chunk_type = [chunk_type[0], chunk_type[1], chunk_type[2], chunk_type[3]];
        let data_end = checked_advance(offset + 8, length, bytes.len(), "PNG chunk data").ok()?;
        let next_offset = checked_advance(data_end, 4, bytes.len(), "PNG chunk crc").ok()?;

        if !PNG_COLOR_SPACE_CHUNK_TYPES.contains(&chunk_type) {
            normalized.extend_from_slice(&bytes[offset..next_offset]);
        }

        offset = next_offset;
    }

    Some(normalized)
}

#[cfg(test)]
const PNG_IDAT_CHUNK_TYPE: [u8; 4] = *b"IDAT";

#[cfg(test)]
fn png_has_chunk(bytes: &[u8], target: [u8; 4]) -> bool {
    if !bytes.starts_with(PNG_SIGNATURE) {
        return false;
    }

    let mut offset = PNG_SIGNATURE.len();
    while offset < bytes.len() {
        let Ok(length) = read_be_u32(bytes, offset, bytes.len(), "PNG chunk length") else {
            return false;
        };
        let Ok(chunk_type) = slice_range(bytes, offset + 4, 4, bytes.len(), "PNG chunk type")
        else {
            return false;
        };
        let chunk_type = [chunk_type[0], chunk_type[1], chunk_type[2], chunk_type[3]];
        if chunk_type == target {
            return true;
        }

        let Ok(data_end) =
            checked_advance(offset + 8, length as usize, bytes.len(), "PNG chunk data")
        else {
            return false;
        };
        let Ok(next_offset) = checked_advance(data_end, 4, bytes.len(), "PNG chunk crc") else {
            return false;
        };
        offset = next_offset;

        if chunk_type == PNG_IDAT_CHUNK_TYPE {
            return false;
        }
    }

    false
}

fn build_png_chunk(chunk_type: [u8; 4], data: &[u8]) -> Vec<u8> {
    let mut chunk = Vec::with_capacity(12 + data.len());
    chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
    chunk.extend_from_slice(&chunk_type);
    chunk.extend_from_slice(data);

    let mut hasher = Hasher::new();
    hasher.update(&chunk_type);
    hasher.update(data);
    chunk.extend_from_slice(&hasher.finalize().to_be_bytes());
    chunk
}

const JPEG_SOI_MARKER: [u8; 2] = [0xFF, 0xD8];
const JPEG_EOI_MARKER: u8 = 0xD9;
const JPEG_SOS_MARKER: u8 = 0xDA;
const JPEG_APP0_MARKER: u8 = 0xE0;
const JPEG_APP15_MARKER: u8 = 0xEF;
const JPEG_APP2_MARKER: u8 = 0xE2;
const JPEG_COM_MARKER: u8 = 0xFE;
const JPEG_ICC_SEGMENT_HEADER: &[u8] = b"ICC_PROFILE\0";
const JPEG_MAX_SEGMENT_DATA_LEN: usize = u16::MAX as usize - 2;
const JPEG_MAX_ICC_CHUNK_LEN: usize = JPEG_MAX_SEGMENT_DATA_LEN - 14;

fn replace_jpeg_icc_profile(bytes: Vec<u8>, icc_profile: &StoredIccProfile) -> Vec<u8> {
    let Some(stripped) = strip_jpeg_icc_segments(&bytes) else {
        return bytes;
    };

    let Some(insert_offset) = jpeg_metadata_insert_offset(&stripped) else {
        return bytes;
    };

    let icc_segments = build_jpeg_icc_segments(&icc_profile.bytes);
    if icc_segments.is_empty() {
        return stripped;
    }

    let total_segment_len = icc_segments.iter().map(Vec::len).sum::<usize>();
    let mut normalized = Vec::with_capacity(stripped.len() + total_segment_len);
    normalized.extend_from_slice(&stripped[..insert_offset]);
    for segment in icc_segments {
        normalized.extend_from_slice(&segment);
    }
    normalized.extend_from_slice(&stripped[insert_offset..]);
    normalized
}

fn strip_jpeg_icc_segments(bytes: &[u8]) -> Option<Vec<u8>> {
    if !bytes.starts_with(&JPEG_SOI_MARKER) {
        return None;
    }

    let mut normalized = Vec::with_capacity(bytes.len());
    normalized.extend_from_slice(&JPEG_SOI_MARKER);
    let mut offset = JPEG_SOI_MARKER.len();

    while offset < bytes.len() {
        let marker = next_jpeg_marker(bytes, offset)?;
        let marker_offset = marker.0;
        let marker_type = marker.1;

        if marker_offset > offset {
            normalized.extend_from_slice(&bytes[offset..marker_offset]);
        }

        if marker_type == JPEG_EOI_MARKER {
            normalized.extend_from_slice(&bytes[marker_offset..marker_offset + 2]);
            offset = marker_offset + 2;
            continue;
        }

        if jpeg_marker_has_no_payload(marker_type) {
            normalized.extend_from_slice(&bytes[marker_offset..marker_offset + 2]);
            offset = marker_offset + 2;
            continue;
        }

        let segment_length =
            read_be_u16(bytes, marker_offset + 2, bytes.len(), "JPEG segment length").ok()?
                as usize;
        if segment_length < 2 {
            return None;
        }
        let segment_end = checked_advance(
            marker_offset + 2,
            segment_length,
            bytes.len(),
            "JPEG segment body",
        )
        .ok()?;

        if marker_type == JPEG_SOS_MARKER {
            normalized.extend_from_slice(&bytes[marker_offset..]);
            break;
        }

        let segment_data = &bytes[marker_offset + 4..segment_end];
        if !(marker_type == JPEG_APP2_MARKER && segment_data.starts_with(JPEG_ICC_SEGMENT_HEADER)) {
            normalized.extend_from_slice(&bytes[marker_offset..segment_end]);
        }

        offset = segment_end;
    }

    Some(normalized)
}

fn build_jpeg_icc_segments(icc_bytes: &[u8]) -> Vec<Vec<u8>> {
    if icc_bytes.is_empty() {
        return Vec::new();
    }

    let total_segments = icc_bytes.len().div_ceil(JPEG_MAX_ICC_CHUNK_LEN);
    if total_segments == 0 || total_segments > u8::MAX as usize {
        return Vec::new();
    }

    icc_bytes
        .chunks(JPEG_MAX_ICC_CHUNK_LEN)
        .enumerate()
        .map(|(index, chunk)| {
            let mut segment_data =
                Vec::with_capacity(JPEG_ICC_SEGMENT_HEADER.len() + chunk.len() + 2);
            segment_data.extend_from_slice(JPEG_ICC_SEGMENT_HEADER);
            segment_data.push((index + 1) as u8);
            segment_data.push(total_segments as u8);
            segment_data.extend_from_slice(chunk);

            let length = (segment_data.len() + 2) as u16;
            let mut segment = Vec::with_capacity(segment_data.len() + 4);
            segment.push(0xFF);
            segment.push(JPEG_APP2_MARKER);
            segment.extend_from_slice(&length.to_be_bytes());
            segment.extend_from_slice(&segment_data);
            segment
        })
        .collect()
}

fn jpeg_metadata_insert_offset(bytes: &[u8]) -> Option<usize> {
    if !bytes.starts_with(&JPEG_SOI_MARKER) {
        return None;
    }

    let mut offset = JPEG_SOI_MARKER.len();
    while offset < bytes.len() {
        let marker = next_jpeg_marker(bytes, offset)?;
        let marker_offset = marker.0;
        let marker_type = marker.1;

        if jpeg_marker_has_no_payload(marker_type) {
            return Some(marker_offset);
        }

        let segment_length =
            read_be_u16(bytes, marker_offset + 2, bytes.len(), "JPEG segment length").ok()?
                as usize;
        if segment_length < 2 {
            return None;
        }
        let segment_end = checked_advance(
            marker_offset + 2,
            segment_length,
            bytes.len(),
            "JPEG segment body",
        )
        .ok()?;

        if marker_type == JPEG_SOS_MARKER {
            return Some(marker_offset);
        }

        if marker_type == JPEG_COM_MARKER
            || (JPEG_APP0_MARKER..=JPEG_APP15_MARKER).contains(&marker_type)
        {
            offset = segment_end;
            continue;
        }

        return Some(marker_offset);
    }

    Some(JPEG_SOI_MARKER.len())
}

fn next_jpeg_marker(bytes: &[u8], offset: usize) -> Option<(usize, u8)> {
    let mut cursor = offset;
    while cursor + 1 < bytes.len() {
        if bytes[cursor] != 0xFF {
            cursor += 1;
            continue;
        }

        let mut marker_cursor = cursor + 1;
        while marker_cursor < bytes.len() && bytes[marker_cursor] == 0xFF {
            marker_cursor += 1;
        }

        if marker_cursor >= bytes.len() {
            return None;
        }

        if bytes[marker_cursor] == 0x00 {
            cursor = marker_cursor + 1;
            continue;
        }

        return Some((cursor, bytes[marker_cursor]));
    }

    None
}

fn jpeg_marker_has_no_payload(marker: u8) -> bool {
    marker == JPEG_EOI_MARKER || marker == 0x01 || (0xD0..=0xD7).contains(&marker) || marker == 0xD8
}

const RIFF_SIGNATURE: &[u8; 4] = b"RIFF";
const WEBP_SIGNATURE: &[u8; 4] = b"WEBP";
const WEBP_VP8X_CHUNK_TYPE: [u8; 4] = *b"VP8X";
const WEBP_VP8L_CHUNK_TYPE: [u8; 4] = *b"VP8L";
const WEBP_ICCP_CHUNK_TYPE: [u8; 4] = *b"ICCP";
const WEBP_VP8X_CHUNK_LEN: usize = 10;
const WEBP_VP8X_ICC_FLAG: u8 = 0b0010_0000;
const WEBP_VP8X_ALPHA_FLAG: u8 = 0b0001_0000;

fn replace_webp_icc_profile(
    bytes: Vec<u8>,
    dimensions: Option<(u32, u32)>,
    icc_profile: &StoredIccProfile,
) -> Vec<u8> {
    let Some(stripped) = strip_webp_iccp_chunks(&bytes) else {
        return bytes;
    };

    let iccp_chunk = build_webp_chunk(WEBP_ICCP_CHUNK_TYPE, &icc_profile.bytes);

    if webp_first_chunk_type(&stripped) == Some(WEBP_VP8X_CHUNK_TYPE) {
        return insert_webp_iccp_into_extended(stripped, &iccp_chunk).unwrap_or(bytes);
    }

    let Some((width, height)) = dimensions else {
        return bytes;
    };

    wrap_simple_webp_with_iccp(stripped, &iccp_chunk, width, height).unwrap_or(bytes)
}

fn strip_webp_iccp_chunks(bytes: &[u8]) -> Option<Vec<u8>> {
    if !bytes.starts_with(RIFF_SIGNATURE)
        || slice_range(bytes, 8, 4, bytes.len(), "WEBP signature").ok()? != WEBP_SIGNATURE
    {
        return None;
    }

    let mut normalized = Vec::with_capacity(bytes.len());
    normalized.extend_from_slice(RIFF_SIGNATURE);
    normalized.extend_from_slice(&[0; 4]);
    normalized.extend_from_slice(WEBP_SIGNATURE);

    let mut offset = 12;
    while offset < bytes.len() {
        let chunk_type = slice_range(bytes, offset, 4, bytes.len(), "WebP chunk type").ok()?;
        let chunk_type = [chunk_type[0], chunk_type[1], chunk_type[2], chunk_type[3]];
        let chunk_length =
            read_le_u32(bytes, offset + 4, bytes.len(), "WebP chunk length").ok()? as usize;
        let padded_length = chunk_length + (chunk_length % 2);
        let chunk_end =
            checked_advance(offset + 8, padded_length, bytes.len(), "WebP chunk body").ok()?;

        if chunk_type != WEBP_ICCP_CHUNK_TYPE {
            normalized.extend_from_slice(&bytes[offset..chunk_end]);
        }

        offset = chunk_end;
    }

    write_webp_riff_size(&mut normalized);
    Some(normalized)
}

fn insert_webp_iccp_into_extended(bytes: Vec<u8>, iccp_chunk: &[u8]) -> Option<Vec<u8>> {
    let vp8x_length = read_le_u32(&bytes, 16, bytes.len(), "WebP VP8X length").ok()? as usize;
    if vp8x_length < WEBP_VP8X_CHUNK_LEN {
        return None;
    }
    let insert_offset = checked_advance(
        20,
        vp8x_length + (vp8x_length % 2),
        bytes.len(),
        "WebP VP8X data",
    )
    .ok()?;

    let mut normalized = Vec::with_capacity(bytes.len() + iccp_chunk.len());
    normalized.extend_from_slice(&bytes[..insert_offset]);
    normalized.extend_from_slice(iccp_chunk);
    normalized.extend_from_slice(&bytes[insert_offset..]);
    let flags = normalized.get_mut(20)?;
    *flags |= WEBP_VP8X_ICC_FLAG;
    write_webp_riff_size(&mut normalized);
    Some(normalized)
}

fn wrap_simple_webp_with_iccp(
    bytes: Vec<u8>,
    iccp_chunk: &[u8],
    width: u32,
    height: u32,
) -> Option<Vec<u8>> {
    if width == 0 || height == 0 {
        return None;
    }

    let first_chunk_type = webp_first_chunk_type(&bytes)?;
    let alpha_flag = if first_chunk_type == WEBP_VP8L_CHUNK_TYPE
        && webp_lossless_has_alpha(&bytes).unwrap_or(false)
    {
        WEBP_VP8X_ALPHA_FLAG
    } else {
        0
    };
    let vp8x_chunk = build_webp_vp8x_chunk(width, height, WEBP_VP8X_ICC_FLAG | alpha_flag);

    let mut normalized = Vec::with_capacity(bytes.len() + vp8x_chunk.len() + iccp_chunk.len());
    normalized.extend_from_slice(RIFF_SIGNATURE);
    normalized.extend_from_slice(&[0; 4]);
    normalized.extend_from_slice(WEBP_SIGNATURE);
    normalized.extend_from_slice(&vp8x_chunk);
    normalized.extend_from_slice(iccp_chunk);
    normalized.extend_from_slice(&bytes[12..]);
    write_webp_riff_size(&mut normalized);
    Some(normalized)
}

fn build_webp_vp8x_chunk(width: u32, height: u32, flags: u8) -> Vec<u8> {
    let mut data = Vec::with_capacity(WEBP_VP8X_CHUNK_LEN);
    data.push(flags);
    data.extend_from_slice(&[0, 0, 0]);
    data.extend_from_slice(&(width - 1).to_le_bytes()[..3]);
    data.extend_from_slice(&(height - 1).to_le_bytes()[..3]);
    build_webp_chunk(WEBP_VP8X_CHUNK_TYPE, &data)
}

fn build_webp_chunk(chunk_type: [u8; 4], data: &[u8]) -> Vec<u8> {
    let padding = data.len() % 2;
    let mut chunk = Vec::with_capacity(8 + data.len() + padding);
    chunk.extend_from_slice(&chunk_type);
    chunk.extend_from_slice(&(data.len() as u32).to_le_bytes());
    chunk.extend_from_slice(data);
    if padding != 0 {
        chunk.push(0);
    }
    chunk
}

fn write_webp_riff_size(bytes: &mut [u8]) {
    if bytes.len() < 8 {
        return;
    }

    let riff_size = (bytes.len() - 8) as u32;
    bytes[4..8].copy_from_slice(&riff_size.to_le_bytes());
}

fn webp_first_chunk_type(bytes: &[u8]) -> Option<[u8; 4]> {
    if !bytes.starts_with(RIFF_SIGNATURE)
        || slice_range(bytes, 8, 4, bytes.len(), "WEBP signature").ok()? != WEBP_SIGNATURE
    {
        return None;
    }

    let chunk_type = slice_range(bytes, 12, 4, bytes.len(), "WebP chunk type").ok()?;
    Some([chunk_type[0], chunk_type[1], chunk_type[2], chunk_type[3]])
}

fn webp_lossless_has_alpha(bytes: &[u8]) -> Option<bool> {
    if webp_first_chunk_type(bytes)? != WEBP_VP8L_CHUNK_TYPE {
        return Some(false);
    }

    let chunk_length = read_le_u32(bytes, 16, bytes.len(), "WebP VP8L length").ok()? as usize;
    if chunk_length < 5 {
        return None;
    }
    let payload = slice_range(bytes, 20, chunk_length, bytes.len(), "WebP VP8L payload").ok()?;
    if payload.first().copied()? != 0x2F {
        return None;
    }
    let bits = u32::from_le_bytes([payload[1], payload[2], payload[3], payload[4]]);
    Some(((bits >> 28) & 0x1) == 1)
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

fn read_le_u32(bytes: &[u8], offset: usize, limit: usize, label: &str) -> Result<u32, String> {
    let slice = slice_range(bytes, offset, 4, limit, label)?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
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
            .generate(&image_path, &OutputResolution::Source, None)
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
                None,
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
            .generate(&image_path, &OutputResolution::Source, None)
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
                None,
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
            .generate(&text_path, &OutputResolution::Source, None)
            .await
            .expect_err("unsupported file should fail");

        assert!(matches!(error, PreviewError::UnsupportedInput { .. }));
    }

    #[tokio::test]
    async fn injects_selected_icc_profile_into_source_png_preview() {
        let dir = tempfile::tempdir().expect("temp dir should exist");
        let image_path = dir.path().join("icc-source.png");
        write_sample_png(&image_path, 96, 64);
        let original_bytes = fs::read(&image_path).expect("png bytes should read");
        let icc_profile = sample_icc_profile();

        let preview = PreviewGenerator
            .generate(&image_path, &OutputResolution::Source, Some(&icc_profile))
            .await
            .expect("png preview should generate");

        assert_ne!(preview.bytes, original_bytes);
        assert!(png_has_chunk(&preview.bytes, PNG_ICCP_CHUNK_TYPE));
        assert!(!png_has_chunk(&preview.bytes, *b"sRGB"));
    }

    #[tokio::test]
    async fn injects_selected_icc_profile_into_resized_png_preview() {
        let dir = tempfile::tempdir().expect("temp dir should exist");
        let image_path = dir.path().join("icc-resized.png");
        write_sample_png(&image_path, 200, 100);
        let icc_profile = sample_icc_profile();

        let preview = PreviewGenerator
            .generate(
                &image_path,
                &OutputResolution::Contain {
                    max_width: 50,
                    max_height: 50,
                },
                Some(&icc_profile),
            )
            .await
            .expect("png preview should resize");

        assert_eq!(preview.width, Some(50));
        assert_eq!(preview.height, Some(25));
        assert!(png_has_chunk(&preview.bytes, PNG_ICCP_CHUNK_TYPE));
    }

    #[tokio::test]
    async fn injects_selected_icc_profile_into_source_jpeg_preview() {
        let dir = tempfile::tempdir().expect("temp dir should exist");
        let image_path = dir.path().join("icc-source.jpg");
        write_sample_jpeg(&image_path, 96, 64);
        let original_bytes = fs::read(&image_path).expect("jpeg bytes should read");
        let icc_profile = sample_icc_profile();

        let preview = PreviewGenerator
            .generate(&image_path, &OutputResolution::Source, Some(&icc_profile))
            .await
            .expect("jpeg preview should generate");

        assert_ne!(preview.bytes, original_bytes);
        assert!(jpeg_has_icc_profile(&preview.bytes));
    }

    #[tokio::test]
    async fn injects_selected_icc_profile_into_resized_jpeg_preview() {
        let dir = tempfile::tempdir().expect("temp dir should exist");
        let image_path = dir.path().join("icc-resized.jpg");
        write_sample_jpeg(&image_path, 200, 100);
        let icc_profile = sample_icc_profile();

        let preview = PreviewGenerator
            .generate(
                &image_path,
                &OutputResolution::Contain {
                    max_width: 50,
                    max_height: 50,
                },
                Some(&icc_profile),
            )
            .await
            .expect("jpeg preview should resize");

        assert_eq!(preview.width, Some(50));
        assert_eq!(preview.height, Some(25));
        assert!(jpeg_has_icc_profile(&preview.bytes));
    }

    #[tokio::test]
    async fn injects_selected_icc_profile_into_source_webp_preview() {
        let dir = tempfile::tempdir().expect("temp dir should exist");
        let image_path = dir.path().join("icc-source.webp");
        write_sample_webp(&image_path, 96, 64);
        let original_bytes = fs::read(&image_path).expect("webp bytes should read");
        let icc_profile = sample_icc_profile();

        let preview = PreviewGenerator
            .generate(&image_path, &OutputResolution::Source, Some(&icc_profile))
            .await
            .expect("webp preview should generate");

        assert_ne!(preview.bytes, original_bytes);
        assert!(webp_has_iccp_chunk(&preview.bytes));
        assert!(webp_has_vp8x_icc_flag(&preview.bytes));
    }

    #[tokio::test]
    async fn injects_selected_icc_profile_into_resized_webp_preview() {
        let dir = tempfile::tempdir().expect("temp dir should exist");
        let image_path = dir.path().join("icc-resized.webp");
        write_sample_webp(&image_path, 200, 100);
        let icc_profile = sample_icc_profile();

        let preview = PreviewGenerator
            .generate(
                &image_path,
                &OutputResolution::Contain {
                    max_width: 50,
                    max_height: 50,
                },
                Some(&icc_profile),
            )
            .await
            .expect("webp preview should resize");

        assert_eq!(preview.width, Some(50));
        assert_eq!(preview.height, Some(25));
        assert!(webp_has_iccp_chunk(&preview.bytes));
        assert!(webp_has_vp8x_icc_flag(&preview.bytes));
    }

    #[tokio::test]
    async fn selected_icc_profile_replaces_existing_png_color_space_metadata() {
        let dir = tempfile::tempdir().expect("temp dir should exist");
        let image_path = dir.path().join("icc-replace.png");
        write_sample_png(&image_path, 96, 64);
        let bytes = fs::read(&image_path).expect("png bytes should read");
        let tagged_bytes = {
            let insert_offset =
                png_ihdr_end_offset(&bytes).expect("sample png should have a valid ihdr");
            let srgb_chunk = build_png_chunk(*b"sRGB", &[0]);
            let mut tagged = Vec::with_capacity(bytes.len() + srgb_chunk.len());
            tagged.extend_from_slice(&bytes[..insert_offset]);
            tagged.extend_from_slice(&srgb_chunk);
            tagged.extend_from_slice(&bytes[insert_offset..]);
            tagged
        };
        fs::write(&image_path, tagged_bytes).expect("tagged png should write");
        let icc_profile = sample_icc_profile();

        let preview = PreviewGenerator
            .generate(&image_path, &OutputResolution::Source, Some(&icc_profile))
            .await
            .expect("png preview should generate");

        assert!(png_has_chunk(&preview.bytes, PNG_ICCP_CHUNK_TYPE));
        assert!(!png_has_chunk(&preview.bytes, *b"sRGB"));
    }

    #[tokio::test]
    async fn selected_icc_profile_replaces_existing_jpeg_icc_metadata() {
        let dir = tempfile::tempdir().expect("temp dir should exist");
        let image_path = dir.path().join("icc-replace.jpg");
        write_sample_jpeg(&image_path, 96, 64);
        let bytes = fs::read(&image_path).expect("jpeg bytes should read");
        let tagged_bytes = replace_jpeg_icc_profile(bytes, &sample_icc_profile());
        fs::write(&image_path, tagged_bytes).expect("tagged jpeg should write");

        let replacement_icc = StoredIccProfile {
            name: "Replacement".to_string(),
            bytes: vec![9, 8, 7, 6],
        };

        let preview = PreviewGenerator
            .generate(
                &image_path,
                &OutputResolution::Source,
                Some(&replacement_icc),
            )
            .await
            .expect("jpeg preview should generate");

        assert_eq!(jpeg_icc_segment_count(&preview.bytes), 1);
    }

    #[tokio::test]
    async fn selected_icc_profile_replaces_existing_webp_icc_metadata() {
        let dir = tempfile::tempdir().expect("temp dir should exist");
        let image_path = dir.path().join("icc-replace.webp");
        write_sample_webp(&image_path, 96, 64);
        let bytes = fs::read(&image_path).expect("webp bytes should read");
        let tagged_bytes = replace_webp_icc_profile(bytes, Some((96, 64)), &sample_icc_profile());
        fs::write(&image_path, tagged_bytes).expect("tagged webp should write");

        let replacement_icc = StoredIccProfile {
            name: "Replacement".to_string(),
            bytes: vec![9, 8, 7, 6],
        };

        let preview = PreviewGenerator
            .generate(
                &image_path,
                &OutputResolution::Source,
                Some(&replacement_icc),
            )
            .await
            .expect("webp preview should generate");

        assert_eq!(webp_iccp_chunk_count(&preview.bytes), 1);
    }

    fn write_sample_png(path: &Path, width: u32, height: u32) {
        let image = DynamicImage::new_rgba8(width, height);
        image.save(path).expect("sample png should save");
    }

    fn write_sample_jpeg(path: &Path, width: u32, height: u32) {
        let image = DynamicImage::new_rgb8(width, height);
        image.save(path).expect("sample jpeg should save");
    }

    fn write_sample_webp(path: &Path, width: u32, height: u32) {
        let image = DynamicImage::new_rgba8(width, height);
        image.save(path).expect("sample webp should save");
    }

    fn sample_icc_profile() -> StoredIccProfile {
        StoredIccProfile {
            name: "LG ULTRAFINE".to_string(),
            bytes: vec![0, 1, 2, 3],
        }
    }

    fn jpeg_has_icc_profile(bytes: &[u8]) -> bool {
        jpeg_icc_segment_count(bytes) > 0
    }

    fn jpeg_icc_segment_count(bytes: &[u8]) -> usize {
        if !bytes.starts_with(&JPEG_SOI_MARKER) {
            return 0;
        }

        let mut count = 0;
        let mut offset = JPEG_SOI_MARKER.len();
        while let Some((marker_offset, marker_type)) = next_jpeg_marker(bytes, offset) {
            if jpeg_marker_has_no_payload(marker_type) {
                offset = marker_offset + 2;
                continue;
            }

            let Ok(segment_length) =
                read_be_u16(bytes, marker_offset + 2, bytes.len(), "JPEG segment length")
            else {
                return count;
            };
            let Ok(segment_end) = checked_advance(
                marker_offset + 2,
                segment_length as usize,
                bytes.len(),
                "JPEG segment body",
            ) else {
                return count;
            };

            if marker_type == JPEG_SOS_MARKER {
                return count;
            }

            let segment_data = &bytes[marker_offset + 4..segment_end];
            if marker_type == JPEG_APP2_MARKER && segment_data.starts_with(JPEG_ICC_SEGMENT_HEADER)
            {
                count += 1;
            }
            offset = segment_end;
        }

        count
    }

    fn webp_has_iccp_chunk(bytes: &[u8]) -> bool {
        webp_iccp_chunk_count(bytes) > 0
    }

    fn webp_iccp_chunk_count(bytes: &[u8]) -> usize {
        let Some(mut offset) = webp_chunk_offset(bytes) else {
            return 0;
        };
        let mut count = 0;

        while offset < bytes.len() {
            let Ok(chunk_type) = slice_range(bytes, offset, 4, bytes.len(), "WebP chunk type")
            else {
                return count;
            };
            let Ok(chunk_length) = read_le_u32(bytes, offset + 4, bytes.len(), "WebP chunk length")
            else {
                return count;
            };
            if chunk_type == WEBP_ICCP_CHUNK_TYPE {
                count += 1;
            }

            let Ok(next_offset) = checked_advance(
                offset + 8,
                chunk_length as usize + (chunk_length as usize % 2),
                bytes.len(),
                "WebP chunk body",
            ) else {
                return count;
            };
            offset = next_offset;
        }

        count
    }

    fn webp_has_vp8x_icc_flag(bytes: &[u8]) -> bool {
        if webp_first_chunk_type(bytes) != Some(WEBP_VP8X_CHUNK_TYPE) {
            return false;
        }

        bytes
            .get(20)
            .map(|flags| flags & WEBP_VP8X_ICC_FLAG != 0)
            .unwrap_or(false)
    }

    fn webp_chunk_offset(bytes: &[u8]) -> Option<usize> {
        if !bytes.starts_with(RIFF_SIGNATURE) {
            return None;
        }
        if slice_range(bytes, 8, 4, bytes.len(), "WEBP signature").ok()? != WEBP_SIGNATURE {
            return None;
        }

        Some(12)
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
