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
}
