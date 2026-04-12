use crate::DisplayIccProfile;
use raw_window_handle::RawWindowHandle;

pub(super) fn list_display_icc_profiles() -> Option<Vec<DisplayIccProfile>> {
    None
}

pub(super) fn window_display_icc_profile(_handle: RawWindowHandle) -> Option<Vec<u8>> {
    None
}
