#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "macos", windows)))]
mod unsupported;
#[cfg(windows)]
mod windows;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayIccProfile {
    pub display_id: String,
    pub display_name: String,
    pub is_primary: bool,
    pub icc_profile: Vec<u8>,
}

pub(crate) fn icc_profile_display_name(bytes: &[u8]) -> Option<String> {
    const ICC_HEADER_LEN: usize = 128;
    const ICC_TAG_TABLE_OFFSET: usize = 128;
    const ICC_TAG_ENTRY_LEN: usize = 12;

    if bytes.len() < ICC_HEADER_LEN + 4 {
        return None;
    }

    let tag_count = read_be_u32(bytes, ICC_TAG_TABLE_OFFSET)?;
    let tag_count = usize::try_from(tag_count).ok()?;

    for index in 0..tag_count {
        let entry_offset = ICC_TAG_TABLE_OFFSET.checked_add(4 + index * ICC_TAG_ENTRY_LEN)?;
        let entry_end = entry_offset.checked_add(ICC_TAG_ENTRY_LEN)?;
        if entry_end > bytes.len() {
            return None;
        }

        let signature = bytes.get(entry_offset..entry_offset + 4)?;
        if signature != b"desc" {
            continue;
        }

        let tag_offset = usize::try_from(read_be_u32(bytes, entry_offset + 4)?).ok()?;
        let tag_size = usize::try_from(read_be_u32(bytes, entry_offset + 8)?).ok()?;
        let tag_end = tag_offset.checked_add(tag_size)?;
        let tag_bytes = bytes.get(tag_offset..tag_end)?;

        if let Some(name) = parse_icc_desc_tag(tag_bytes).or_else(|| parse_icc_mluc_tag(tag_bytes))
        {
            return Some(name);
        }
    }

    None
}

fn parse_icc_desc_tag(tag_bytes: &[u8]) -> Option<String> {
    if tag_bytes.len() < 12 || tag_bytes.get(..4)? != b"desc" {
        return None;
    }

    let ascii_length = usize::try_from(read_be_u32(tag_bytes, 8)?).ok()?;
    if ascii_length == 0 {
        return None;
    }

    let ascii_end = 12usize.checked_add(ascii_length)?;
    let ascii_bytes = tag_bytes.get(12..ascii_end)?;
    let ascii_bytes = ascii_bytes
        .strip_suffix(&[0])
        .unwrap_or(ascii_bytes)
        .iter()
        .copied()
        .take_while(|byte| *byte != 0)
        .collect::<Vec<_>>();
    let name = String::from_utf8(ascii_bytes).ok()?;
    let name = name.trim();

    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn parse_icc_mluc_tag(tag_bytes: &[u8]) -> Option<String> {
    if tag_bytes.len() < 16 || tag_bytes.get(..4)? != b"mluc" {
        return None;
    }

    let record_count = usize::try_from(read_be_u32(tag_bytes, 8)?).ok()?;
    let record_size = usize::try_from(read_be_u32(tag_bytes, 12)?).ok()?;
    if record_count == 0 || record_size < 12 {
        return None;
    }

    for index in 0..record_count {
        let record_offset = 16usize.checked_add(index * record_size)?;
        let record_end = record_offset.checked_add(12)?;
        if record_end > tag_bytes.len() {
            return None;
        }

        let string_length = usize::try_from(read_be_u32(tag_bytes, record_offset + 4)?).ok()?;
        let string_offset = usize::try_from(read_be_u32(tag_bytes, record_offset + 8)?).ok()?;
        let string_end = string_offset.checked_add(string_length)?;
        let string_bytes = tag_bytes.get(string_offset..string_end)?;
        if string_bytes.len() % 2 != 0 {
            continue;
        }

        let utf16 = string_bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        let name = String::from_utf16(&utf16).ok()?;
        let name = name.trim();
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }

    None
}

fn read_be_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset + 4)?;
    Some(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

/// Returns the ICC profiles for the active displays visible to the process.
///
/// On macOS this enumerates active CoreGraphics displays. On Windows this
/// enumerates monitors attached to the desktop. Per-display lookup failures are
/// skipped, and platform or enumeration failures return `None`.
pub fn list_display_icc_profiles() -> Option<Vec<DisplayIccProfile>> {
    imp::list_display_icc_profiles()
}

/// Returns the ICC profile bytes for the main or primary display.
///
/// On macOS this resolves the main display's current color space. On Windows
/// this resolves the primary monitor profile. All lookup failures return
/// `None`.
pub fn current_display_icc_profile() -> Option<Vec<u8>> {
    list_display_icc_profiles()?
        .into_iter()
        .find(|profile| profile.is_primary)
        .map(|profile| profile.icc_profile)
}

/// Returns the ICC profile bytes for the display hosting the provided window.
///
/// On macOS, AppKit-backed window lookups only succeed on the main thread.
/// All lookup failures return `None`.
pub fn window_display_icc_profile<W: HasWindowHandle + ?Sized>(window: &W) -> Option<Vec<u8>> {
    let handle = window.window_handle().ok()?;
    window_display_icc_profile_for_raw_handle(handle.as_raw())
}

/// Returns the ICC profile bytes for the display hosting the provided raw
/// window handle.
///
/// On macOS, AppKit-backed window lookups only succeed on the main thread.
/// All lookup failures return `None`.
pub fn window_display_icc_profile_for_raw_handle(handle: RawWindowHandle) -> Option<Vec<u8>> {
    imp::window_display_icc_profile(handle)
}

#[cfg(target_os = "macos")]
use macos as imp;
#[cfg(not(any(target_os = "macos", windows)))]
use unsupported as imp;
#[cfg(windows)]
use windows as imp;

#[cfg(test)]
mod tests {
    use raw_window_handle::RawWindowHandle;

    use super::*;

    #[test]
    fn current_display_lookup_does_not_panic() {
        let _ = current_display_icc_profile();
    }

    #[test]
    fn display_enumeration_does_not_panic() {
        let _ = list_display_icc_profiles();
    }

    #[test]
    fn unsupported_raw_handle_returns_none() {
        #[cfg(target_os = "macos")]
        let handle = {
            let Some(hwnd) = std::num::NonZeroIsize::new(1) else {
                panic!("non-zero handle should construct");
            };
            RawWindowHandle::Win32(raw_window_handle::Win32WindowHandle::new(hwnd))
        };

        #[cfg(windows)]
        let handle = {
            let view_ptr = std::ptr::NonNull::<std::ffi::c_void>::dangling();
            RawWindowHandle::AppKit(raw_window_handle::AppKitWindowHandle::new(view_ptr))
        };

        #[cfg(not(any(target_os = "macos", windows)))]
        let handle = {
            let view_ptr = std::ptr::NonNull::<std::ffi::c_void>::dangling();
            RawWindowHandle::AppKit(raw_window_handle::AppKitWindowHandle::new(view_ptr))
        };

        assert!(window_display_icc_profile_for_raw_handle(handle).is_none());
    }

    #[test]
    fn extracts_icc_profile_name_from_desc_tag() {
        let bytes = sample_desc_icc_profile("LG ULTRAFINE");

        assert_eq!(
            icc_profile_display_name(&bytes).as_deref(),
            Some("LG ULTRAFINE")
        );
    }

    #[test]
    fn extracts_icc_profile_name_from_mluc_tag() {
        let bytes = sample_mluc_icc_profile("Studio Display");

        assert_eq!(
            icc_profile_display_name(&bytes).as_deref(),
            Some("Studio Display")
        );
    }

    fn sample_desc_icc_profile(name: &str) -> Vec<u8> {
        let ascii_len = name.len() + 1;
        let tag_len = 12 + ascii_len;
        let tag_offset = 128 + 4 + 12;
        let total_len = tag_offset + tag_len;
        let mut bytes = vec![0; total_len];
        bytes[128..132].copy_from_slice(&1u32.to_be_bytes());
        bytes[132..136].copy_from_slice(b"desc");
        bytes[136..140].copy_from_slice(&(tag_offset as u32).to_be_bytes());
        bytes[140..144].copy_from_slice(&(tag_len as u32).to_be_bytes());
        bytes[tag_offset..tag_offset + 4].copy_from_slice(b"desc");
        bytes[tag_offset + 8..tag_offset + 12].copy_from_slice(&(ascii_len as u32).to_be_bytes());
        bytes[tag_offset + 12..tag_offset + 12 + name.len()].copy_from_slice(name.as_bytes());
        bytes
    }

    fn sample_mluc_icc_profile(name: &str) -> Vec<u8> {
        let utf16 = name.encode_utf16().collect::<Vec<_>>();
        let string_len = utf16.len() * 2;
        let record_size = 12usize;
        let string_offset = 16 + record_size;
        let tag_len = string_offset + string_len;
        let tag_offset = 128 + 4 + 12;
        let total_len = tag_offset + tag_len;
        let mut bytes = vec![0; total_len];
        bytes[128..132].copy_from_slice(&1u32.to_be_bytes());
        bytes[132..136].copy_from_slice(b"desc");
        bytes[136..140].copy_from_slice(&(tag_offset as u32).to_be_bytes());
        bytes[140..144].copy_from_slice(&(tag_len as u32).to_be_bytes());
        bytes[tag_offset..tag_offset + 4].copy_from_slice(b"mluc");
        bytes[tag_offset + 8..tag_offset + 12].copy_from_slice(&1u32.to_be_bytes());
        bytes[tag_offset + 12..tag_offset + 16]
            .copy_from_slice(&(record_size as u32).to_be_bytes());
        bytes[tag_offset + 16..tag_offset + 18].copy_from_slice(b"en");
        bytes[tag_offset + 18..tag_offset + 20].copy_from_slice(b"US");
        bytes[tag_offset + 20..tag_offset + 24].copy_from_slice(&(string_len as u32).to_be_bytes());
        bytes[tag_offset + 24..tag_offset + 28]
            .copy_from_slice(&(string_offset as u32).to_be_bytes());
        let mut cursor = tag_offset + string_offset;
        for code_unit in utf16 {
            bytes[cursor..cursor + 2].copy_from_slice(&code_unit.to_be_bytes());
            cursor += 2;
        }
        bytes
    }
}
