//! Проверка извлечения иконок: `cargo run -p tm-core --example icontest <path>`.

use std::ffi::c_void;

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS,
};
use windows_sys::Win32::UI::Shell::ExtractIconExW;
use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| r"C:\Windows\explorer.exe".to_string());
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let mut large: HICON = std::ptr::null_mut();
    let mut small: HICON = std::ptr::null_mut();
    let count = unsafe { ExtractIconExW(wide.as_ptr(), 0, &mut large, &mut small, 1) };
    println!("ExtractIconEx count={count} large={large:?} small={small:?}");
    let icon = if !small.is_null() { small } else { large };
    if icon.is_null() {
        return;
    }

    let mut info: ICONINFO = unsafe { std::mem::zeroed() };
    let ok = unsafe { GetIconInfo(icon, &mut info) };
    println!("GetIconInfo ok={ok} color={:?} mask={:?}", info.hbmColor, info.hbmMask);

    let mut bm: BITMAP = unsafe { std::mem::zeroed() };
    let got = unsafe {
        GetObjectW(
            info.hbmColor,
            std::mem::size_of::<BITMAP>() as i32,
            &mut bm as *mut _ as *mut c_void,
        )
    };
    println!("GetObjectW={got} size={}x{}", bm.bmWidth, bm.bmHeight);

    let dc = unsafe { GetDC(std::ptr::null_mut()) };
    let mut bmi: BITMAPINFO = unsafe { std::mem::zeroed() };
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = bm.bmWidth;
    bmi.bmiHeader.biHeight = -bm.bmHeight;
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;
    let mut buf = vec![0u8; (bm.bmWidth * bm.bmHeight * 4) as usize];
    let lines = unsafe {
        GetDIBits(
            dc,
            info.hbmColor,
            0,
            bm.bmHeight as u32,
            buf.as_mut_ptr() as *mut c_void,
            &mut bmi,
            DIB_RGB_COLORS,
        )
    };
    println!("GetDIBits lines={lines} first_px={:?}", &buf[..16.min(buf.len())]);
    let alpha_nonzero = buf.chunks_exact(4).filter(|p| p[3] != 0).count();
    println!("alpha_nonzero={alpha_nonzero} total={}", buf.len() / 4);

    unsafe {
        ReleaseDC(std::ptr::null_mut(), dc);
        DeleteObject(info.hbmColor as HANDLE);
        DeleteObject(info.hbmMask as HANDLE);
        if !small.is_null() {
            DestroyIcon(small);
        }
        if !large.is_null() {
            DestroyIcon(large);
        }
    }
}
