#![allow(non_snake_case, dead_code, unsafe_op_in_unsafe_fn)]

use std::ffi::c_void;
use std::fs;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use wpg::WpgDecoder;

type HRESULT = i32;
type HBITMAP = *mut c_void;
type HINSTANCE = *mut c_void;

const S_OK: HRESULT = 0;
const S_FALSE: HRESULT = 1;
const E_NOINTERFACE: HRESULT = 0x80004002_u32 as i32;
const E_POINTER: HRESULT = 0x80004003_u32 as i32;
const E_FAIL: HRESULT = 0x80004005_u32 as i32;
const CLASS_E_NOAGGREGATION: HRESULT = 0x80040110_u32 as i32;

const DIB_RGB_COLORS: u32 = 0;
const WTSAT_ARGB: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct GUID {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

// {6F0E9720-3051-4E78-8319-5A676BE466EC}
pub const CLSID_WPG_THUMBNAIL_PROVIDER: GUID = GUID {
    data1: 0x6f0e9720,
    data2: 0x3051,
    data3: 0x4e78,
    data4: [0x83, 0x19, 0x5a, 0x67, 0x6b, 0xe4, 0x66, 0xec],
};

// {00000000-0000-0000-C000-000000000046}
pub const IID_IUNKNOWN: GUID = GUID {
    data1: 0x00000000,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

// {00000001-0000-0000-C000-000000000046}
pub const IID_ICLASSFACTORY: GUID = GUID {
    data1: 0x00000001,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

// {E357FCCD-A995-4576-B01F-234630154E96}
pub const IID_ITHUMBNAILPROVIDER: GUID = GUID {
    data1: 0xe357fccd,
    data2: 0xa995,
    data3: 0x4576,
    data4: [0xb0, 0x1f, 0x23, 0x46, 0x30, 0x15, 0x4e, 0x96],
};

// {B7D14566-0509-4CCE-A71F-0A554233BD98}
pub const IID_IINITIALIZEWITHFILE: GUID = GUID {
    data1: 0xb7d14566,
    data2: 0x0509,
    data3: 0x4cce,
    data4: [0xa7, 0x1f, 0x0a, 0x55, 0x42, 0x33, 0xbd, 0x98],
};

// {B824B49D-22AC-4161-AC8A-9916E8FA3F7F}
pub const IID_IINITIALIZEWITHSTREAM: GUID = GUID {
    data1: 0xb824b49d,
    data2: 0x22ac,
    data3: 0x4161,
    data4: [0xac, 0x8a, 0x99, 0x16, 0xe8, 0xfa, 0x3f, 0x7f],
};

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

#[link(name = "gdi32")]
#[link(name = "shell32")]
#[link(name = "kernel32")]
#[link(name = "advapi32")]
unsafe extern "system" {
    fn CreateDIBSection(
        hdc: *mut c_void,
        pbmi: *const BITMAPINFO,
        usage: u32,
        ppvBits: *mut *mut c_void,
        hSection: *mut c_void,
        offset: u32,
    ) -> HBITMAP;
    fn GetModuleFileNameW(hModule: HINSTANCE, lpFilename: *mut u16, nSize: u32) -> u32;
    fn SHChangeNotify(wEventId: i32, uFlags: u32, dwItem1: *const c_void, dwItem2: *const c_void);
}

static mut G_MODULE: HINSTANCE = null_mut();
static G_LOCK_COUNT: AtomicU32 = AtomicU32::new(0);

// COM Interface definitions
#[repr(C)]
struct IUnknownVtbl {
    QueryInterface: unsafe extern "system" fn(this: *mut c_void, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT,
    AddRef: unsafe extern "system" fn(this: *mut c_void) -> u32,
    Release: unsafe extern "system" fn(this: *mut c_void) -> u32,
}

#[repr(C)]
struct IClassFactoryVtbl {
    parent: IUnknownVtbl,
    CreateInstance: unsafe extern "system" fn(this: *mut c_void, pUnkOuter: *mut c_void, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT,
    LockServer: unsafe extern "system" fn(this: *mut c_void, fLock: i32) -> HRESULT,
}

#[repr(C)]
struct IThumbnailProviderVtbl {
    parent: IUnknownVtbl,
    GetBitmap: unsafe extern "system" fn(this: *mut c_void, cx: u32, phbmp: *mut HBITMAP, pdwAlpha: *mut u32) -> HRESULT,
}

#[repr(C)]
struct IInitializeWithFileVtbl {
    parent: IUnknownVtbl,
    Initialize: unsafe extern "system" fn(this: *mut c_void, pszFilePath: *const u16, grfMode: u32) -> HRESULT,
}

#[repr(C)]
struct IInitializeWithStreamVtbl {
    parent: IUnknownVtbl,
    Initialize: unsafe extern "system" fn(this: *mut c_void, pstream: *mut c_void, grfMode: u32) -> HRESULT,
}

#[repr(C)]
struct IStreamVtbl {
    parent_qi: unsafe extern "system" fn(this: *mut c_void, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT,
    parent_addref: unsafe extern "system" fn(this: *mut c_void) -> u32,
    parent_release: unsafe extern "system" fn(this: *mut c_void) -> u32,
    Read: unsafe extern "system" fn(this: *mut c_void, pv: *mut c_void, cb: u32, pcbRead: *mut u32) -> HRESULT,
    Write: unsafe extern "system" fn(this: *mut c_void, pv: *const c_void, cb: u32, pcbWritten: *mut u32) -> HRESULT,
}

// ---------------------------------------------------------------------------
// WpgThumbnailProvider Object Implementation
// ---------------------------------------------------------------------------
#[repr(C)]
struct WpgThumbnailProvider {
    thumb_vtbl: *const IThumbnailProviderVtbl,
    file_vtbl: *const IInitializeWithFileVtbl,
    stream_vtbl: *const IInitializeWithStreamVtbl,
    ref_count: AtomicU32,
    raw_data: Mutex<Option<Vec<u8>>>,
}

impl WpgThumbnailProvider {
    fn new() -> *mut Self {
        G_LOCK_COUNT.fetch_add(1, Ordering::SeqCst);
        let obj = Box::new(WpgThumbnailProvider {
            thumb_vtbl: &THUMB_VTBL,
            file_vtbl: &FILE_VTBL,
            stream_vtbl: &STREAM_VTBL,
            ref_count: AtomicU32::new(1),
            raw_data: Mutex::new(None),
        });
        Box::into_raw(obj)
    }
}

// Global Vtables for WpgThumbnailProvider
static THUMB_VTBL: IThumbnailProviderVtbl = IThumbnailProviderVtbl {
    parent: IUnknownVtbl {
        QueryInterface: thumb_qi,
        AddRef: thumb_addref,
        Release: thumb_release,
    },
    GetBitmap: thumb_get_bitmap,
};

static FILE_VTBL: IInitializeWithFileVtbl = IInitializeWithFileVtbl {
    parent: IUnknownVtbl {
        QueryInterface: file_qi,
        AddRef: file_addref,
        Release: file_release,
    },
    Initialize: file_initialize,
};

static STREAM_VTBL: IInitializeWithStreamVtbl = IInitializeWithStreamVtbl {
    parent: IUnknownVtbl {
        QueryInterface: stream_qi,
        AddRef: stream_addref,
        Release: stream_release,
    },
    Initialize: stream_initialize,
};

// IThumbnailProvider methods
unsafe extern "system" fn thumb_qi(this: *mut c_void, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
    let self_ptr = this as *mut WpgThumbnailProvider;
    common_qi(self_ptr, riid, ppv)
}

unsafe extern "system" fn thumb_addref(this: *mut c_void) -> u32 {
    let self_ptr = this as *mut WpgThumbnailProvider;
    (*self_ptr).ref_count.fetch_add(1, Ordering::SeqCst) + 1
}

unsafe extern "system" fn thumb_release(this: *mut c_void) -> u32 {
    let self_ptr = this as *mut WpgThumbnailProvider;
    let count = (*self_ptr).ref_count.fetch_sub(1, Ordering::SeqCst) - 1;
    if count == 0 {
        G_LOCK_COUNT.fetch_sub(1, Ordering::SeqCst);
        drop(Box::from_raw(self_ptr));
    }
    count
}

unsafe extern "system" fn thumb_get_bitmap(
    this: *mut c_void,
    cx: u32,
    phbmp: *mut HBITMAP,
    pdwAlpha: *mut u32,
) -> HRESULT {
    if phbmp.is_null() || pdwAlpha.is_null() {
        return E_POINTER;
    }
    *phbmp = null_mut();
    *pdwAlpha = 0;

    let self_ptr = this as *mut WpgThumbnailProvider;
    let guard = (*self_ptr).raw_data.lock().unwrap();
    let data = match guard.as_ref() {
        Some(d) => d,
        None => return E_FAIL,
    };

    // Decode WPG bitstream
    let decoded = match WpgDecoder::decode(data) {
        Ok(dec) => dec,
        Err(_) => return E_FAIL,
    };

    let src_w = decoded.width as f64;
    let src_h = decoded.height as f64;
    let max_cx = (cx.max(32) as f64).min(1024.0);

    // Compute aspect-ratio preserved target dimensions
    let scale = (max_cx / src_w).min(max_cx / src_h).min(1.0);
    let target_w = (src_w * scale).round().max(1.0) as u32;
    let target_h = (src_h * scale).round().max(1.0) as u32;

    let mut bgra = vec![0u8; (target_w * target_h * 4) as usize];

    if target_w == decoded.width && target_h == decoded.height {
        // Direct pixel copy with RGBA -> BGRA swap
        for (src_chunk, dst_chunk) in decoded.rgba.chunks_exact(4).zip(bgra.chunks_exact_mut(4)) {
            dst_chunk[0] = src_chunk[2]; // B
            dst_chunk[1] = src_chunk[1]; // G
            dst_chunk[2] = src_chunk[0]; // R
            dst_chunk[3] = src_chunk[3]; // A
        }
    } else {
        // High quality fast downsampling
        for y in 0..target_h {
            let src_y = ((y as f64 / target_h as f64) * src_h).min(src_h - 1.0) as usize;
            for x in 0..target_w {
                let src_x = ((x as f64 / target_w as f64) * src_w).min(src_w - 1.0) as usize;
                let src_idx = (src_y * decoded.width as usize + src_x) * 4;
                let dst_idx = (y as usize * target_w as usize + x as usize) * 4;

                bgra[dst_idx] = decoded.rgba[src_idx + 2];     // B
                bgra[dst_idx + 1] = decoded.rgba[src_idx + 1]; // G
                bgra[dst_idx + 2] = decoded.rgba[src_idx];     // R
                bgra[dst_idx + 3] = decoded.rgba[src_idx + 3]; // A
            }
        }
    }

    // Allocate 32-bit top-down GDI DIBSection
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: target_w as i32,
            biHeight: -(target_h as i32), // Top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: 0, // BI_RGB
            biSizeImage: (target_w * target_h * 4),
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [0],
    };

    let mut bits: *mut c_void = null_mut();
    let hbmp = CreateDIBSection(null_mut(), &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
    if hbmp.is_null() || bits.is_null() {
        return E_FAIL;
    }

    // Copy BGRA bytes to GDI buffer
    std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits as *mut u8, bgra.len());

    *phbmp = hbmp;
    *pdwAlpha = WTSAT_ARGB; // Full alpha channel support
    S_OK
}

// IInitializeWithFile methods
unsafe extern "system" fn file_qi(this: *mut c_void, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
    let self_ptr = (this as *mut u8).sub(std::mem::size_of::<usize>()) as *mut WpgThumbnailProvider;
    common_qi(self_ptr, riid, ppv)
}

unsafe extern "system" fn file_addref(this: *mut c_void) -> u32 {
    let self_ptr = (this as *mut u8).sub(std::mem::size_of::<usize>()) as *mut WpgThumbnailProvider;
    thumb_addref(self_ptr as *mut c_void)
}

unsafe extern "system" fn file_release(this: *mut c_void) -> u32 {
    let self_ptr = (this as *mut u8).sub(std::mem::size_of::<usize>()) as *mut WpgThumbnailProvider;
    thumb_release(self_ptr as *mut c_void)
}

unsafe extern "system" fn file_initialize(this: *mut c_void, pszFilePath: *const u16, _grfMode: u32) -> HRESULT {
    if pszFilePath.is_null() {
        return E_POINTER;
    }
    let mut len = 0;
    while *pszFilePath.add(len) != 0 {
        len += 1;
    }
    let slice = std::slice::from_raw_parts(pszFilePath, len);
    let path_str = String::from_utf16_lossy(slice);

    let self_ptr = (this as *mut u8).sub(std::mem::size_of::<usize>()) as *mut WpgThumbnailProvider;
    if let Ok(bytes) = fs::read(PathBuf::from(path_str)) {
        *(*self_ptr).raw_data.lock().unwrap() = Some(bytes);
        S_OK
    } else {
        E_FAIL
    }
}

// IInitializeWithStream methods
unsafe extern "system" fn stream_qi(this: *mut c_void, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
    let self_ptr = (this as *mut u8).sub(std::mem::size_of::<usize>() * 2) as *mut WpgThumbnailProvider;
    common_qi(self_ptr, riid, ppv)
}

unsafe extern "system" fn stream_addref(this: *mut c_void) -> u32 {
    let self_ptr = (this as *mut u8).sub(std::mem::size_of::<usize>() * 2) as *mut WpgThumbnailProvider;
    thumb_addref(self_ptr as *mut c_void)
}

unsafe extern "system" fn stream_release(this: *mut c_void) -> u32 {
    let self_ptr = (this as *mut u8).sub(std::mem::size_of::<usize>() * 2) as *mut WpgThumbnailProvider;
    thumb_release(self_ptr as *mut c_void)
}

unsafe extern "system" fn stream_initialize(this: *mut c_void, pstream: *mut c_void, _grfMode: u32) -> HRESULT {
    if pstream.is_null() {
        return E_POINTER;
    }

    let stream_vtbl = *(pstream as *const *const IStreamVtbl);
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 65536];

    loop {
        let mut read_bytes: u32 = 0;
        let hr = ((*stream_vtbl).Read)(pstream, chunk.as_mut_ptr() as *mut c_void, chunk.len() as u32, &mut read_bytes);
        if hr != S_OK || read_bytes == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read_bytes as usize]);
    }

    let self_ptr = (this as *mut u8).sub(std::mem::size_of::<usize>() * 2) as *mut WpgThumbnailProvider;
    if !buffer.is_empty() {
        *(*self_ptr).raw_data.lock().unwrap() = Some(buffer);
        S_OK
    } else {
        E_FAIL
    }
}

// Common QueryInterface for WpgThumbnailProvider
unsafe fn common_qi(self_ptr: *mut WpgThumbnailProvider, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
    if ppv.is_null() {
        return E_POINTER;
    }
    *ppv = null_mut();

    let guid = *riid;
    if guid == IID_IUNKNOWN || guid == IID_ITHUMBNAILPROVIDER {
        *ppv = &mut (*self_ptr).thumb_vtbl as *mut _ as *mut c_void;
    } else if guid == IID_IINITIALIZEWITHFILE {
        *ppv = &mut (*self_ptr).file_vtbl as *mut _ as *mut c_void;
    } else if guid == IID_IINITIALIZEWITHSTREAM {
        *ppv = &mut (*self_ptr).stream_vtbl as *mut _ as *mut c_void;
    } else {
        return E_NOINTERFACE;
    }

    thumb_addref(&mut (*self_ptr).thumb_vtbl as *mut _ as *mut c_void);
    S_OK
}

// ---------------------------------------------------------------------------
// ClassFactory Implementation
// ---------------------------------------------------------------------------
#[repr(C)]
struct WpgClassFactory {
    vtbl: *const IClassFactoryVtbl,
    ref_count: AtomicU32,
}

static CF_VTBL: IClassFactoryVtbl = IClassFactoryVtbl {
    parent: IUnknownVtbl {
        QueryInterface: cf_qi,
        AddRef: cf_addref,
        Release: cf_release,
    },
    CreateInstance: cf_create_instance,
    LockServer: cf_lock_server,
};

unsafe extern "system" fn cf_qi(this: *mut c_void, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
    if ppv.is_null() {
        return E_POINTER;
    }
    *ppv = null_mut();

    let guid = *riid;
    if guid == IID_IUNKNOWN || guid == IID_ICLASSFACTORY {
        *ppv = this;
        cf_addref(this);
        S_OK
    } else {
        E_NOINTERFACE
    }
}

unsafe extern "system" fn cf_addref(this: *mut c_void) -> u32 {
    let self_ptr = this as *mut WpgClassFactory;
    (*self_ptr).ref_count.fetch_add(1, Ordering::SeqCst) + 1
}

unsafe extern "system" fn cf_release(this: *mut c_void) -> u32 {
    let self_ptr = this as *mut WpgClassFactory;
    let count = (*self_ptr).ref_count.fetch_sub(1, Ordering::SeqCst) - 1;
    if count == 0 {
        drop(Box::from_raw(self_ptr));
    }
    count
}

unsafe extern "system" fn cf_create_instance(
    _this: *mut c_void,
    pUnkOuter: *mut c_void,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    if !pUnkOuter.is_null() {
        return CLASS_E_NOAGGREGATION;
    }
    let instance = WpgThumbnailProvider::new();
    let hr = thumb_qi(instance as *mut c_void, riid, ppv);
    thumb_release(instance as *mut c_void);
    hr
}

unsafe extern "system" fn cf_lock_server(_this: *mut c_void, fLock: i32) -> HRESULT {
    if fLock != 0 {
        G_LOCK_COUNT.fetch_add(1, Ordering::SeqCst);
    } else {
        G_LOCK_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
    S_OK
}

// ---------------------------------------------------------------------------
// COM DLL Exports
// ---------------------------------------------------------------------------
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllMain(hinst: HINSTANCE, reason: u32, _reserved: *mut c_void) -> i32 {
    if reason == 1 {
        // DLL_PROCESS_ATTACH
        G_MODULE = hinst;
    }
    1
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    if ppv.is_null() {
        return E_POINTER;
    }
    *ppv = null_mut();

    if *rclsid != CLSID_WPG_THUMBNAIL_PROVIDER {
        return 0x80040111_u32 as i32; // CLASS_E_CLASSNOTAVAILABLE
    }

    let cf = Box::new(WpgClassFactory {
        vtbl: &CF_VTBL,
        ref_count: AtomicU32::new(1),
    });

    let hr = cf_qi(Box::into_raw(cf) as *mut c_void, riid, ppv);
    hr
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllCanUnloadNow() -> HRESULT {
    if G_LOCK_COUNT.load(Ordering::SeqCst) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllRegisterServer() -> HRESULT {
    let mut path_buf = [0u16; 1024];
    let len = GetModuleFileNameW(G_MODULE, path_buf.as_mut_ptr(), 1024);
    if len == 0 {
        return E_FAIL;
    }
    let dll_path = String::from_utf16_lossy(&path_buf[..len as usize]);

    let reg_script = format!(
        r#"
$hkcu = "HKCU:\Software\Classes"
$clsid = "{{6F0E9720-3051-4E78-8319-5A676BE466EC}}"
$thumbIid = "{{E357FCCD-A995-4576-B01F-234630154E96}}"

# Register CLSID
New-Item -Path "$hkcu\CLSID\$clsid" -Force | Out-Null
Set-ItemProperty -Path "$hkcu\CLSID\$clsid" -Name "(Default)" -Value "WPG Thumbnail Provider"
Set-ItemProperty -Path "$hkcu\CLSID\$clsid" -Name "DisableProcessIsolation" -Value 1 -Type DWord

New-Item -Path "$hkcu\CLSID\$clsid\InProcServer32" -Force | Out-Null
Set-ItemProperty -Path "$hkcu\CLSID\$clsid\InProcServer32" -Name "(Default)" -Value '{dll}'
Set-ItemProperty -Path "$hkcu\CLSID\$clsid\InProcServer32" -Name "ThreadingModel" -Value "Apartment"

# Associate with .wpg extension ShellEx
New-Item -Path "$hkcu\.wpg\ShellEx\$thumbIid" -Force | Out-Null
Set-ItemProperty -Path "$hkcu\.wpg\ShellEx\$thumbIid" -Name "(Default)" -Value "$clsid"

# Associate with WPGImage ProgID ShellEx
New-Item -Path "$hkcu\WPGImage\ShellEx\$thumbIid" -Force | Out-Null
Set-ItemProperty -Path "$hkcu\WPGImage\ShellEx\$thumbIid" -Name "(Default)" -Value "$clsid"

# Refresh Explorer Shell
$code = '[DllImport("shell32.dll")]public static extern void SHChangeNotify(int wEventId, int uFlags, IntPtr dwItem1, IntPtr dwItem2);'
$type = Add-Type -MemberDefinition $code -Name ShellThumbUtil -Namespace Win32 -PassThru
$type::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)
"#,
        dll = dll_path
    );

    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &reg_script])
        .status();

    match status {
        Ok(s) if s.success() => {
            SHChangeNotify(0x08000000, 0, null(), null());
            S_OK
        }
        _ => E_FAIL,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllUnregisterServer() -> HRESULT {
    let unreg_script = r#"
$clsid = "{6F0E9720-3051-4E78-8319-5A676BE466EC}"
$thumbIid = "{E357FCCD-A995-4576-B01F-234630154E96}"

Remove-Item -Path "HKCU:\Software\Classes\CLSID\$clsid" -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -Path "HKCU:\Software\Classes\.wpg\ShellEx\$thumbIid" -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -Path "HKCU:\Software\Classes\WPGImage\ShellEx\$thumbIid" -Recurse -Force -ErrorAction SilentlyContinue

$code = '[DllImport("shell32.dll")]public static extern void SHChangeNotify(int wEventId, int uFlags, IntPtr dwItem1, IntPtr dwItem2);'
$type = Add-Type -MemberDefinition $code -Name ShellThumbUtilUnreg -Namespace Win32 -PassThru
$type::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)
"#;
    let _ = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", unreg_script])
        .status();

    SHChangeNotify(0x08000000, 0, null(), null());
    S_OK
}
