use super::*;
use std::ffi::CString;

#[test]
fn desktop_options_ffi_rejects_invalid_buffers_before_launch() {
    unsafe {
        assert!(launch_options_from_ffi(std::ptr::null(), 0).is_err());
        assert!(
            launch_options_from_ffi(std::ptr::dangling(), preparation::MAX_OPTIONS_BYTES + 1)
                .is_err()
        );
        assert!(launch_options_from_ffi(b"{}".as_ptr(), 2)
            .unwrap()
            .plugins
            .is_empty());
        assert!(launch_options_from_ffi(b"{".as_ptr(), 1).is_err());
        assert!(rfvp_pump_create(std::ptr::null(), std::ptr::null()).is_null());
        assert!(
            rfvp_pump_create_v1(std::ptr::null(), std::ptr::null(), b"{}".as_ptr(), 2).is_null()
        );
        assert_eq!(
            rfvp_run_entry_v1(std::ptr::null(), std::ptr::null(), b"{}".as_ptr(), 2),
            2
        );
        let root = CString::new("unused").unwrap();
        let nls = CString::new("sjis").unwrap();
        assert_eq!(
            rfvp_run_entry_v1(root.as_ptr(), nls.as_ptr(), b"{".as_ptr(), 1),
            2
        );
        assert!(rfvp_pump_create_v1(root.as_ptr(), nls.as_ptr(), b"{".as_ptr(), 1).is_null());
    }
}
