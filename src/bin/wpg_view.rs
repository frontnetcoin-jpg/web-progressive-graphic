#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(non_snake_case, dead_code)]

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::time::Instant;
use wpg::{WpgDecoder, WpgHeader};

// Win32 Types and Constants
type HWND = *mut std::ffi::c_void;
type HDC = *mut std::ffi::c_void;
type HBITMAP = *mut std::ffi::c_void;
type HBRUSH = *mut std::ffi::c_void;
type HCURSOR = *mut std::ffi::c_void;
type HICON = *mut std::ffi::c_void;
type HINSTANCE = *mut std::ffi::c_void;
type HMENU = *mut std::ffi::c_void;
type LPARAM = isize;
type WPARAM = usize;
type LRESULT = isize;
type HDROP = *mut std::ffi::c_void;

const WM_DESTROY: u32 = 0x0002;
const WM_SIZE: u32 = 0x0005;
const WM_PAINT: u32 = 0x000F;
const WM_ERASEBKGND: u32 = 0x0014;
const WM_KEYDOWN: u32 = 0x0100;
const WM_MOUSEMOVE: u32 = 0x0200;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_LBUTTONDBLCLK: u32 = 0x0203;
const WM_MOUSEWHEEL: u32 = 0x020A;
const WM_DROPFILES: u32 = 0x0233;

const WS_OVERLAPPEDWINDOW: u32 = 0x00CF0000;
const WS_VISIBLE: u32 = 0x10000000;
const WS_POPUP: u32 = 0x80000000;

const CS_HREDRAW: u32 = 0x0002;
const CS_VREDRAW: u32 = 0x0001;
const CS_DBLCLKS: u32 = 0x0008;

const SW_SHOW: i32 = 5;
const COLOR_WINDOW: u32 = 5;
const CW_USEDEFAULT: i32 = 0x80000000_u32 as i32;

const VK_ESCAPE: usize = 0x1B;
const VK_SPACE: usize = 0x20;
const VK_PRIOR: usize = 0x21; // Page Up
const VK_NEXT: usize = 0x22;  // Page Down
const VK_LEFT: usize = 0x25;
const VK_UP: usize = 0x26;
const VK_RIGHT: usize = 0x27;
const VK_DOWN: usize = 0x28;
const VK_0: usize = 0x30;
const VK_1: usize = 0x31;
const VK_F: usize = 0x46;
const VK_I: usize = 0x49;
const VK_S: usize = 0x53;
const VK_F11: usize = 0x7A;

const DIB_RGB_COLORS: u32 = 0;
const SRCCOPY: u32 = 0x00CC0020;
const COLORONCOLOR: i32 = 3;
const HALFTONE: i32 = 4;
const DT_LEFT: u32 = 0x00000000;
const DT_WORDBREAK: u32 = 0x00000010;

#[repr(C)]
struct WNDCLASSEXW {
    cbSize: u32,
    style: u32,
    lpfnWndProc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
    cbClsExtra: i32,
    cbWndExtra: i32,
    hInstance: HINSTANCE,
    hIcon: HICON,
    hCursor: HCURSOR,
    hbrBackground: HBRUSH,
    lpszMenuName: *const u16,
    lpszClassName: *const u16,
    hIconSm: HICON,
}

#[repr(C)]
struct POINT {
    x: i32,
    y: i32,
}

#[repr(C)]
struct MSG {
    hwnd: HWND,
    message: u32,
    wParam: WPARAM,
    lParam: LPARAM,
    time: u32,
    pt: POINT,
}

#[repr(C)]
struct RECT {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
struct PAINTSTRUCT {
    hdc: HDC,
    fErase: i32,
    rcPaint: RECT,
    fRestore: i32,
    fIncUpdate: i32,
    rgbReserved: [u8; 32],
}

#[repr(C)]
struct BITMAPINFOHEADER {
    biSize: u32,
    biWidth: i32,
    biHeight: i32,
    biPlanes: u16,
    biBitCount: u16,
    biCompression: u32,
    biSizeImage: u32,
    biXPelsPerMeter: i32,
    biYPelsPerMeter: i32,
    biClrUsed: u32,
    biClrImportant: u32,
}

#[repr(C)]
struct BITMAPINFO {
    bmiHeader: BITMAPINFOHEADER,
    bmiColors: [u32; 1],
}

#[link(name = "user32")]
#[link(name = "gdi32")]
#[link(name = "shell32")]
#[link(name = "dwmapi")]
unsafe extern "system" {
    fn GetModuleHandleW(lpModuleName: *const u16) -> HINSTANCE;
    fn RegisterClassExW(lpWndClass: *const WNDCLASSEXW) -> u16;
    fn CreateWindowExW(
        dwExStyle: u32,
        lpClassName: *const u16,
        lpWindowName: *const u16,
        dwStyle: u32,
        X: i32,
        Y: i32,
        nWidth: i32,
        nHeight: i32,
        hWndParent: HWND,
        hMenu: HMENU,
        hInstance: HINSTANCE,
        lpParam: *mut std::ffi::c_void,
    ) -> HWND;
    fn ShowWindow(hWnd: HWND, nCmdShow: i32) -> i32;
    fn UpdateWindow(hWnd: HWND) -> i32;
    fn GetMessageW(lpMsg: *mut MSG, hWnd: HWND, wMsgFilterMin: u32, wMsgFilterMax: u32) -> i32;
    fn TranslateMessage(lpMsg: *const MSG) -> i32;
    fn DispatchMessageW(lpMsg: *const MSG) -> LRESULT;
    fn PostQuitMessage(nExitCode: i32);
    fn DefWindowProcW(hWnd: HWND, Msg: u32, wParam: WPARAM, lParam: LPARAM) -> LRESULT;
    fn GetClientRect(hWnd: HWND, lpRect: *mut RECT) -> i32;
    fn InvalidateRect(hWnd: HWND, lpRect: *const RECT, bErase: i32) -> i32;
    fn BeginPaint(hWnd: HWND, lpPaint: *mut PAINTSTRUCT) -> HDC;
    fn EndPaint(hWnd: HWND, lpPaint: *const PAINTSTRUCT) -> i32;
    fn SetWindowTextW(hWnd: HWND, lpString: *const u16) -> i32;
    fn LoadCursorW(hInstance: HINSTANCE, lpCursorName: *const u16) -> HCURSOR;
    fn SetWindowLongPtrW(hWnd: HWND, nIndex: i32, dwNewLong: isize) -> isize;
    fn GetWindowLongPtrW(hWnd: HWND, nIndex: i32) -> isize;
    fn DragAcceptFiles(hWnd: HWND, fAccept: i32);
    fn DragQueryFileW(hDrop: HDROP, iFile: u32, lpszFile: *mut u16, cch: u32) -> u32;
    fn DragFinish(hDrop: HDROP);
    fn GetSystemMetrics(nIndex: i32) -> i32;
    fn SetWindowPos(
        hWnd: HWND,
        hWndInsertAfter: HWND,
        X: i32,
        Y: i32,
        cx: i32,
        cy: i32,
        uFlags: u32,
    ) -> i32;

    fn CreateCompatibleDC(hdc: HDC) -> HDC;
    fn CreateCompatibleBitmap(hdc: HDC, cx: i32, cy: i32) -> HBITMAP;
    fn SelectObject(hdc: HDC, h: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    fn DeleteDC(hdc: HDC) -> i32;
    fn DeleteObject(ho: *mut std::ffi::c_void) -> i32;
    fn BitBlt(
        hdc: HDC,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        hdcSrc: HDC,
        x1: i32,
        y1: i32,
        rop: u32,
    ) -> i32;
    fn SetStretchBltMode(hdc: HDC, mode: i32) -> i32;
    fn StretchDIBits(
        hdc: HDC,
        xDest: i32,
        yDest: i32,
        DestWidth: i32,
        DestHeight: i32,
        xSrc: i32,
        ySrc: i32,
        SrcWidth: i32,
        SrcHeight: i32,
        lpBits: *const std::ffi::c_void,
        lpbmi: *const BITMAPINFO,
        iUsage: u32,
        rop: u32,
    ) -> i32;
    fn CreateSolidBrush(color: u32) -> HBRUSH;
    fn FillRect(hDC: HDC, lprc: *const RECT, hbr: HBRUSH) -> i32;
    fn SetTextColor(hdc: HDC, color: u32) -> u32;
    fn SetBkMode(hdc: HDC, mode: i32) -> i32;
    fn DrawTextW(
        hdc: HDC,
        lpchText: *const u16,
        cchText: i32,
        lprc: *mut RECT,
        format: u32,
    ) -> i32;
    fn DwmSetWindowAttribute(
        hwnd: HWND,
        dwAttribute: u32,
        pvAttribute: *const std::ffi::c_void,
        cbAttribute: u32,
    ) -> i32;
}

fn to_wstring(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

struct LoadedImage {
    path: PathBuf,
    width: u32,
    height: u32,
    bgra: Vec<u8>,
    file_size_bytes: usize,
    decode_time_ms: f64,
    mode_str: String,
    subsampling_str: String,
    has_preview: bool,
    has_grain: bool,
    quality: u8,
}

struct ViewerState {
    hwnd: HWND,
    image: Option<LoadedImage>,
    current_dir_wpgs: Vec<PathBuf>,
    current_index: usize,
    zoom: f64,
    pan_x: f64,
    pan_y: f64,
    is_dragging: bool,
    drag_start_x: i32,
    drag_start_y: i32,
    drag_pan_orig_x: f64,
    drag_pan_orig_y: f64,
    show_hud: bool,
    is_fullscreen: bool,
    saved_rect: RECT,
    saved_style: u32,
}

impl ViewerState {
    fn new(hwnd: HWND) -> Self {
        Self {
            hwnd,
            image: None,
            current_dir_wpgs: Vec::new(),
            current_index: 0,
            zoom: 1.0,
            pan_x: 0.0,
            pan_y: 0.0,
            is_dragging: false,
            drag_start_x: 0,
            drag_start_y: 0,
            drag_pan_orig_x: 0.0,
            drag_pan_orig_y: 0.0,
            show_hud: true,
            is_fullscreen: false,
            saved_rect: RECT { left: 0, top: 0, right: 0, bottom: 0 },
            saved_style: 0,
        }
    }

    fn scan_directory(&mut self, current_file: &Path) {
        if let Some(parent) = current_file.parent() {
            let mut list = Vec::new();
            if let Ok(entries) = fs::read_dir(parent) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_file() {
                        if let Some(ext) = p.extension() {
                            if ext.to_string_lossy().eq_ignore_ascii_case("wpg") {
                                list.push(p);
                            }
                        }
                    }
                }
            }
            list.sort();
            self.current_index = list.iter().position(|p| p == current_file).unwrap_or(0);
            self.current_dir_wpgs = list;
        }
    }

    fn load_file(&mut self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let bytes = fs::read(path)?;
        let mut cursor = std::io::Cursor::new(&bytes);
        let header = WpgHeader::deserialize(&mut cursor)?;

        let t0 = Instant::now();
        let decoded = WpgDecoder::decode(&bytes)?;
        let decode_time_ms = t0.elapsed().as_secs_f64() * 1000.0;

        // Convert RGBA to BGRA for high-speed Windows GDI DIB
        let mut bgra = decoded.rgba;
        for chunk in bgra.chunks_exact_mut(4) {
            chunk.swap(0, 2);
        }

        let mode_str = format!("{:?}", header.mode);
        let subsampling_str = format!("{:?}", header.subsampling);

        let img = LoadedImage {
            path: path.to_path_buf(),
            width: decoded.width,
            height: decoded.height,
            bgra,
            file_size_bytes: bytes.len(),
            decode_time_ms,
            mode_str,
            subsampling_str,
            has_preview: header.has_micro_preview(),
            has_grain: header.has_film_grain(),
            quality: header.quality,
        };

        self.scan_directory(path);
        self.image = Some(img);
        self.reset_view();
        self.update_title();
        unsafe { InvalidateRect(self.hwnd, null(), 0) };
        Ok(())
    }

    fn reset_view(&mut self) {
        let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        unsafe { GetClientRect(self.hwnd, &mut rc) };
        let win_w = (rc.right - rc.left).max(1) as f64;
        let win_h = (rc.bottom - rc.top).max(1) as f64;

        if let Some(img) = &self.image {
            let img_w = img.width as f64;
            let img_h = img.height as f64;

            // Fit with a comfortable margin
            let fit_zoom = ((win_w * 0.95) / img_w).min((win_h * 0.95) / img_h);
            self.zoom = fit_zoom.min(1.0); // If smaller than window, start at 100%

            let display_w = img_w * self.zoom;
            let display_h = img_h * self.zoom;

            self.pan_x = (win_w - display_w) / 2.0;
            self.pan_y = (win_h - display_h) / 2.0;
        } else {
            self.zoom = 1.0;
            self.pan_x = 0.0;
            self.pan_y = 0.0;
        }
    }

    fn update_title(&self) {
        let title = if let Some(img) = &self.image {
            let filename = img.path.file_name().unwrap_or_default().to_string_lossy();
            let kb = img.file_size_bytes as f64 / 1024.0;
            let bpp = (img.file_size_bytes as f64 * 8.0) / (img.width * img.height) as f64;
            format!(
                "WPG Viewer — {} ({}x{} px | {:.1} KB | {:.3} bpp | {:.1} ms) [{}/{}]",
                filename,
                img.width,
                img.height,
                kb,
                bpp,
                img.decode_time_ms,
                if self.current_dir_wpgs.is_empty() { 1 } else { self.current_index + 1 },
                self.current_dir_wpgs.len().max(1)
            )
        } else {
            "WPG Viewer — Next-Gen Web Progressive Graphic (Drag & Drop any .wpg file)".to_string()
        };

        let w_title = to_wstring(&title);
        unsafe { SetWindowTextW(self.hwnd, w_title.as_ptr()) };
    }

    fn navigate(&mut self, delta: isize) {
        if self.current_dir_wpgs.is_empty() {
            return;
        }
        let len = self.current_dir_wpgs.len() as isize;
        let next_idx = ((self.current_index as isize + delta) % len + len) % len;
        let path = self.current_dir_wpgs[next_idx as usize].clone();
        let _ = self.load_file(&path);
    }

    fn save_as_png(&self) {
        if let Some(img) = &self.image {
            let mut png_path = img.path.clone();
            png_path.set_extension("png");

            // Convert BGRA back to RGBA for saving
            let mut rgba = img.bgra.clone();
            for chunk in rgba.chunks_exact_mut(4) {
                chunk.swap(0, 2);
            }

            if let Some(buf) = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(img.width, img.height, rgba) {
                if buf.save(&png_path).is_ok() {
                    let title = format!("Saved snapshot to: {}", png_path.display());
                    let w_title = to_wstring(&title);
                    unsafe { SetWindowTextW(self.hwnd, w_title.as_ptr()) };
                }
            }
        }
    }

    fn toggle_fullscreen(&mut self) {
        self.is_fullscreen = !self.is_fullscreen;
        if self.is_fullscreen {
            unsafe {
                GetClientRect(self.hwnd, &mut self.saved_rect);
                self.saved_style = GetWindowLongPtrW(self.hwnd, -16) as u32; // GWL_STYLE
                SetWindowLongPtrW(self.hwnd, -16, (WS_VISIBLE | WS_POPUP) as isize);
                let screen_w = GetSystemMetrics(0); // SM_CXSCREEN
                let screen_h = GetSystemMetrics(1); // SM_CYSCREEN
                SetWindowPos(self.hwnd, null_mut(), 0, 0, screen_w, screen_h, 0x0040); // SWP_SHOWWINDOW
            }
        } else {
            unsafe {
                SetWindowLongPtrW(self.hwnd, -16, self.saved_style as isize);
                SetWindowPos(
                    self.hwnd,
                    null_mut(),
                    100,
                    100,
                    1100,
                    750,
                    0x0040,
                );
            }
        }
        self.reset_view();
        unsafe { InvalidateRect(self.hwnd, null(), 0) };
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        let ptr = GetWindowLongPtrW(hwnd, -21) as *mut ViewerState; // GWLP_USERDATA

        match msg {
            WM_SIZE => {
                if !ptr.is_null() {
                    InvalidateRect(hwnd, null(), 0);
                }
                0
            }

            WM_ERASEBKGND => 1, // Suppress flickering, double-buffered in WM_PAINT

            WM_PAINT => {
                let mut ps = std::mem::zeroed::<PAINTSTRUCT>();
                let hdc = BeginPaint(hwnd, &mut ps);

                let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
                GetClientRect(hwnd, &mut rc);
                let win_w = rc.right - rc.left;
                let win_h = rc.bottom - rc.top;

                // Create off-screen memory DC for silky smooth double buffering
                let mem_dc = CreateCompatibleDC(hdc);
                let mem_bmp = CreateCompatibleBitmap(hdc, win_w, win_h);
                let old_bmp = SelectObject(mem_dc, mem_bmp);

                // Sleek dark-mode neutral background: #12141a
                let bg_brush = CreateSolidBrush(0x1a1412); // 0xBBGGRR
                FillRect(mem_dc, &rc, bg_brush);
                DeleteObject(bg_brush);

                if !ptr.is_null() {
                    let state = &mut *ptr;

                    if let Some(img) = &state.image {
                        let dest_x = state.pan_x.round() as i32;
                        let dest_y = state.pan_y.round() as i32;
                        let dest_w = (img.width as f64 * state.zoom).round() as i32;
                        let dest_h = (img.height as f64 * state.zoom).round() as i32;

                        let mut bmi = std::mem::zeroed::<BITMAPINFO>();
                        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
                        bmi.bmiHeader.biWidth = img.width as i32;
                        bmi.bmiHeader.biHeight = -(img.height as i32); // Top-down DIB
                        bmi.bmiHeader.biPlanes = 1;
                        bmi.bmiHeader.biBitCount = 32;
                        bmi.bmiHeader.biCompression = 0; // BI_RGB

                        SetStretchBltMode(mem_dc, if state.zoom < 1.0 { HALFTONE } else { COLORONCOLOR });

                        StretchDIBits(
                            mem_dc,
                            dest_x,
                            dest_y,
                            dest_w,
                            dest_h,
                            0,
                            0,
                            img.width as i32,
                            img.height as i32,
                            img.bgra.as_ptr() as *const std::ffi::c_void,
                            &bmi,
                            DIB_RGB_COLORS,
                            SRCCOPY,
                        );

                        // HUD Information Overlay
                        if state.show_hud {
                            let hud_w = 420;
                            let hud_h = 175;
                            let hud_rc = RECT {
                                left: 16,
                                top: 16,
                                right: 16 + hud_w,
                                bottom: 16 + hud_h,
                            };

                            let hud_brush = CreateSolidBrush(0x221814); // Deep translucent-feel dark box
                            FillRect(mem_dc, &hud_rc, hud_brush);
                            DeleteObject(hud_brush);

                            let bpp = (img.file_size_bytes as f64 * 8.0) / (img.width * img.height) as f64;
                            let kb = img.file_size_bytes as f64 / 1024.0;
                            let orig_raw_kb = (img.width * img.height * 4) as f64 / 1024.0;
                            let ratio = orig_raw_kb / kb.max(0.1);

                            let info_text = format!(
                                " 🖼️  File: {}\n 📐  Resolution: {} x {} px\n 📦  Size: {:.1} KB ({:.3} bpp, {:.1}:1 ratio)\n ⚡  Decode: {:.1} ms | Quality: Q{}\n 🎨  Mode: {} | Chroma: {}\n 🎬  Film Grain: {} | Micro-Preview: {}\n 🔍  Zoom: {:.0}% (Hotkeys: Wheel, Drag, I, F, 0, S)",
                                img.path.file_name().unwrap_or_default().to_string_lossy(),
                                img.width,
                                img.height,
                                kb,
                                bpp,
                                ratio,
                                img.decode_time_ms,
                                img.quality,
                                img.mode_str,
                                img.subsampling_str,
                                if img.has_grain { "Active" } else { "None" },
                                if img.has_preview { "16-Byte" } else { "None" },
                                state.zoom * 100.0,
                            );

                            let mut text_rc = RECT {
                                left: 24,
                                top: 22,
                                right: 16 + hud_w - 8,
                                bottom: 16 + hud_h - 6,
                            };

                            SetBkMode(mem_dc, 1); // TRANSPARENT
                            SetTextColor(mem_dc, 0x00f3f4f6); // Soft white text
                            let w_info = to_wstring(&info_text);
                            DrawTextW(mem_dc, w_info.as_ptr(), -1, &mut text_rc, DT_LEFT | DT_WORDBREAK);
                        }
                    } else {
                        // Empty state welcome placeholder
                        let msg = "✨ WPG (Web Progressive Graphic) Native Viewer\n\n📁 Drag & drop any .wpg file here to open\n\n⌨️ Hotkeys:\n • Mouse Wheel: Zoom in / out\n • Left Drag: Pan image\n • Space / Arrows: Next / Previous image\n • I: Toggle Info HUD\n • F / F11: Fullscreen\n • 0: Reset Zoom & Pan\n • S: Save decoded image as PNG";
                        let mut text_rc = RECT {
                            left: win_w / 2 - 250,
                            top: win_h / 2 - 120,
                            right: win_w / 2 + 250,
                            bottom: win_h / 2 + 160,
                        };

                        SetBkMode(mem_dc, 1);
                        SetTextColor(mem_dc, 0x00a5b4fc);
                        let w_msg = to_wstring(msg);
                        DrawTextW(mem_dc, w_msg.as_ptr(), -1, &mut text_rc, DT_LEFT | DT_WORDBREAK);
                    }
                }

                // Blit final double-buffered frame to display
                BitBlt(hdc, 0, 0, win_w, win_h, mem_dc, 0, 0, SRCCOPY);

                SelectObject(mem_dc, old_bmp);
                DeleteObject(mem_bmp);
                DeleteDC(mem_dc);
                EndPaint(hwnd, &ps);
                0
            }

            WM_MOUSEWHEEL => {
                if !ptr.is_null() {
                    let state = &mut *ptr;
                    let delta = ((wparam >> 16) as i16) as f64;
                    let factor = if delta > 0.0 { 1.15 } else { 1.0 / 1.15 };

                    let cursor_x = (lparam & 0xFFFF) as i16 as f64;
                    let cursor_y = ((lparam >> 16) & 0xFFFF) as i16 as f64;

                    let old_zoom = state.zoom;
                    state.zoom = (state.zoom * factor).clamp(0.02, 50.0);

                    // Zoom centered on cursor position
                    let scale = state.zoom / old_zoom;
                    state.pan_x = cursor_x - (cursor_x - state.pan_x) * scale;
                    state.pan_y = cursor_y - (cursor_y - state.pan_y) * scale;

                    InvalidateRect(hwnd, null(), 0);
                }
                0
            }

            WM_LBUTTONDOWN => {
                if !ptr.is_null() {
                    let state = &mut *ptr;
                    state.is_dragging = true;
                    state.drag_start_x = (lparam & 0xFFFF) as i16 as i32;
                    state.drag_start_y = ((lparam >> 16) & 0xFFFF) as i16 as i32;
                    state.drag_pan_orig_x = state.pan_x;
                    state.drag_pan_orig_y = state.pan_y;
                }
                0
            }

            WM_LBUTTONUP => {
                if !ptr.is_null() {
                    let state = &mut *ptr;
                    state.is_dragging = false;
                }
                0
            }

            WM_MOUSEMOVE => {
                if !ptr.is_null() {
                    let state = &mut *ptr;
                    if state.is_dragging {
                        let cur_x = (lparam & 0xFFFF) as i16 as i32;
                        let cur_y = ((lparam >> 16) & 0xFFFF) as i16 as i32;
                        let dx = (cur_x - state.drag_start_x) as f64;
                        let dy = (cur_y - state.drag_start_y) as f64;
                        state.pan_x = state.drag_pan_orig_x + dx;
                        state.pan_y = state.drag_pan_orig_y + dy;
                        InvalidateRect(hwnd, null(), 0);
                    }
                }
                0
            }

            WM_LBUTTONDBLCLK => {
                if !ptr.is_null() {
                    let state = &mut *ptr;
                    state.reset_view();
                    InvalidateRect(hwnd, null(), 0);
                }
                0
            }

            WM_KEYDOWN => {
                if !ptr.is_null() {
                    let state = &mut *ptr;
                    match wparam {
                        VK_ESCAPE => {
                            if state.is_fullscreen {
                                state.toggle_fullscreen();
                            } else {
                                PostQuitMessage(0);
                            }
                        }
                        VK_0 => {
                            state.reset_view();
                            InvalidateRect(hwnd, null(), 0);
                        }
                        VK_1 => {
                            state.zoom = 1.0;
                            InvalidateRect(hwnd, null(), 0);
                        }
                        VK_F | VK_F11 => {
                            state.toggle_fullscreen();
                        }
                        VK_I => {
                            state.show_hud = !state.show_hud;
                            InvalidateRect(hwnd, null(), 0);
                        }
                        VK_S => {
                            state.save_as_png();
                        }
                        VK_RIGHT | VK_NEXT | VK_SPACE => {
                            state.navigate(1);
                        }
                        VK_LEFT | VK_PRIOR => {
                            state.navigate(-1);
                        }
                        _ => (),
                    }
                }
                0
            }

            WM_DROPFILES => {
                let hdrop = wparam as HDROP;
                let mut file_buf = [0u16; 1024];
                let len = DragQueryFileW(hdrop, 0, file_buf.as_mut_ptr(), 1024);
                if len > 0 && !ptr.is_null() {
                    let path_str = String::from_utf16_lossy(&file_buf[..len as usize]);
                    let state = &mut *ptr;
                    let _ = state.load_file(Path::new(&path_str));
                }
                DragFinish(hdrop);
                0
            }

            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }

            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let initial_file = args.get(1).map(PathBuf::from);

    unsafe {
        let instance = GetModuleHandleW(null());
        let class_name = to_wstring("WpgImageViewerClass");

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
            lpfnWndProc: wnd_proc,
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: null_mut(),
            hCursor: LoadCursorW(null_mut(), 32512 as *const u16), // IDC_ARROW
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class_name.as_ptr(),
            hIconSm: null_mut(),
        };

        RegisterClassExW(&wc);

        let window_title = to_wstring("WPG Viewer — Next-Gen Web Progressive Graphic");
        let hwnd = CreateWindowExW(
            0x00000010, // WS_EX_ACCEPTFILES
            class_name.as_ptr(),
            window_title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1100,
            750,
            null_mut(),
            null_mut(),
            instance,
            null_mut(),
        );

        if hwnd.is_null() {
            eprintln!("Failed to create window");
            return;
        }

        // Enable Windows 10/11 immersive dark mode titlebar
        let dark_mode: i32 = 1;
        DwmSetWindowAttribute(hwnd, 20, &dark_mode as *const _ as *const _, 4);

        DragAcceptFiles(hwnd, 1);

        let mut state = Box::new(ViewerState::new(hwnd));
        if let Some(path) = initial_file {
            let _ = state.load_file(&path);
        } else {
            state.update_title();
        }

        SetWindowLongPtrW(hwnd, -21, Box::into_raw(state) as isize); // GWLP_USERDATA

        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);

        let mut msg = std::mem::zeroed::<MSG>();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // Clean up state
        let state_ptr = GetWindowLongPtrW(hwnd, -21) as *mut ViewerState;
        if !state_ptr.is_null() {
            drop(Box::from_raw(state_ptr));
        }
    }
}
