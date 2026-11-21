//! Иконки процессов: извлекаем 16×16 из exe, лениво и с кэшем текстур.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::Arc;

use eframe::egui::{self, Color32, ColorImage, TextureHandle, TextureOptions};
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HDC,
};
use windows_sys::Win32::UI::Shell::ExtractIconExW;
use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

#[derive(Default)]
pub struct IconCache {
    map: HashMap<Arc<str>, Option<TextureHandle>>,
}

impl IconCache {
    pub fn get(
        &mut self,
        ctx: &egui::Context,
        path: &Arc<str>,
    ) -> Option<egui::TextureId> {
        if let Some(entry) = self.map.get(path) {
            return entry.as_ref().map(|t| t.id());
        }
        let image = load_icon(path);
        let handle = image.map(|img| {
            ctx.load_texture(
                format!("icon:{path}"),
                img,
                TextureOptions::LINEAR,
            )
        });
        let id = handle.as_ref().map(|t| t.id());
        self.map.insert(path.clone(), handle);
        id
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn loaded(&self) -> usize {
        self.map.values().filter(|v| v.is_some()).count()
    }
}

fn load_icon(path: &str) -> Option<ColorImage> {
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let mut large: HICON = std::ptr::null_mut();
    let mut small: HICON = std::ptr::null_mut();
    let count = unsafe { ExtractIconExW(wide.as_ptr(), 0, &mut large, &mut small, 1) };
    if count == 0 || (large.is_null() && small.is_null()) {
        debug_icon(path, "ExtractIconExW: значок не найден");
        return None;
    }
    let icon = if !small.is_null() { small } else { large };
    let image = hicon_to_image(icon);
    if image.is_none() {
        debug_icon(path, "не удалось преобразовать HICON");
    }
    unsafe {
        if !small.is_null() {
            DestroyIcon(small);
        }
        if !large.is_null() {
            DestroyIcon(large);
        }
    }
    image
}

fn debug_icon(path: &str, message: &str) {
    if std::env::var_os("TM_DEBUG_ICONS").is_some() {
        eprintln!("icon {path}: {message}");
    }
}

fn hicon_to_image(icon: HICON) -> Option<ColorImage> {
    let mut info: ICONINFO = unsafe { std::mem::zeroed() };
    if unsafe { GetIconInfo(icon, &mut info) } == 0 {
        return None;
    }
    let result = bitmap_to_image(info.hbmColor);
    unsafe {
        if !info.hbmColor.is_null() {
            DeleteObject(info.hbmColor as HANDLE);
        }
        if !info.hbmMask.is_null() {
            DeleteObject(info.hbmMask as HANDLE);
        }
    }
    result
}

fn bitmap_to_image(bitmap: HANDLE) -> Option<ColorImage> {
    if bitmap.is_null() {
        return None;
    }
    let mut bm: BITMAP = unsafe { std::mem::zeroed() };
    let got = unsafe {
        GetObjectW(
            bitmap,
            std::mem::size_of::<BITMAP>() as i32,
            &mut bm as *mut _ as *mut c_void,
        )
    };
    if got == 0 {
        return None;
    }
    let w = bm.bmWidth.max(1);
    let h = bm.bmHeight.max(1);
    if w > 256 || h > 256 {
        return None;
    }

    let dc: HDC = unsafe { GetDC(std::ptr::null_mut()) };
    if dc.is_null() {
        return None;
    }

    let mut bmi: BITMAPINFO = unsafe { std::mem::zeroed() };
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = w;
    bmi.bmiHeader.biHeight = -h; // top-down
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;

    let mut buf = vec![0u8; (w * h * 4) as usize];
    let lines = unsafe {
        GetDIBits(
            dc,
            bitmap,
            0,
            h as u32,
            buf.as_mut_ptr() as *mut c_void,
            &mut bmi,
            DIB_RGB_COLORS,
        )
    };
    unsafe {
        ReleaseDC(std::ptr::null_mut(), dc);
    }
    if lines == 0 {
        return None;
    }

    let has_alpha = buf.chunks_exact(4).any(|px| px[3] != 0);
    let mut pixels = Vec::with_capacity((w * h) as usize);
    for px in buf.chunks_exact(4) {
        let b = px[0];
        let g = px[1];
        let r = px[2];
        let a = if has_alpha { px[3] } else { 255 };
        pixels.push(Color32::from_rgba_unmultiplied(r, g, b, a));
    }
    Some(ColorImage { size: [w as usize, h as usize], pixels })
}
