use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::UI::Controls::Dialogs::{
    ChooseColorW, CHOOSECOLORW, CC_ANYCOLOR, CC_FULLOPEN, CC_RGBINIT,
};

static mut CUSTOM_COLORS: [COLORREF; 16] = [COLORREF(0); 16];

pub fn choose_color(initial_r: u8, initial_g: u8, initial_b: u8) -> Option<(u8, u8, u8)> {
    unsafe {
        let initial_ref = COLORREF((initial_r as u32) | ((initial_g as u32) << 8) | ((initial_b as u32) << 16));
        let mut cc = CHOOSECOLORW {
            lStructSize: std::mem::size_of::<CHOOSECOLORW>() as u32,
            hwndOwner: HWND::default(),
            hInstance: HWND::default(),
            rgbResult: initial_ref,
            lpCustColors: std::ptr::addr_of_mut!(CUSTOM_COLORS) as *mut COLORREF,
            Flags: CC_RGBINIT | CC_FULLOPEN | CC_ANYCOLOR,
            lCustData: windows::Win32::Foundation::LPARAM(0),
            lpfnHook: None,
            lpTemplateName: windows::core::PCWSTR::null(),
        };

        if ChooseColorW(&mut cc).as_bool() {
            let col = cc.rgbResult.0;
            let r = (col & 0xFF) as u8;
            let g = ((col >> 8) & 0xFF) as u8;
            let b = ((col >> 16) & 0xFF) as u8;
            Some((r, g, b))
        } else {
            None
        }
    }
}
