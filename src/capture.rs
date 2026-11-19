//! A window screenshot, pixel for pixel, for the share sheet.

#[cfg(target_os = "macos")]
pub(crate) fn window_png() -> Result<Vec<u8>, String> {
    use nsh_platform_macos::{
        objc2::MainThreadMarker,
        objc2_app_kit::{NSApplication, NSBitmapImageFileType},
        objc2_foundation::NSDictionary,
    };

    let mtm = MainThreadMarker::new().ok_or("window capture must run on the main thread")?;
    let app = NSApplication::sharedApplication(mtm);
    let usable = |window: &nsh_platform_macos::objc2_app_kit::NSWindow| {
        window.isVisible() && window.contentView().is_some()
    };
    let mut largest: Option<(
        f64,
        nsh_platform_macos::objc2::rc::Retained<nsh_platform_macos::objc2_app_kit::NSWindow>,
    )> = None;
    for window in app.windows().iter() {
        let Some(view) = window.contentView() else {
            continue;
        };
        if !window.isVisible() {
            continue;
        }
        let size = view.bounds().size;
        let area = size.width * size.height;
        if area < 10_000.0 {
            continue;
        }
        if largest.as_ref().is_none_or(|(best, _)| area > *best) {
            largest = Some((area, window));
        }
    }
    let window = app
        .mainWindow()
        .or_else(|| app.keyWindow())
        .filter(|window| usable(window))
        .or_else(|| largest.map(|(_, window)| window))
        .ok_or("the app has no visible window to capture")?;
    let view = window
        .contentView()
        .ok_or("the app window has no content view")?;
    let bounds = view.bounds();
    let rep = view
        .bitmapImageRepForCachingDisplayInRect(bounds)
        .ok_or("the window could not make a bitmap")?;
    view.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
    let properties = NSDictionary::new();
    let data = unsafe {
        rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &properties)
    }
    .ok_or("the window bitmap could not be encoded as PNG")?;
    Ok(data.to_vec())
}

#[cfg(target_os = "windows")]
pub(crate) fn window_png() -> Result<Vec<u8>, String> {
    use std::ffi::c_void;
    use windows::core::BOOL;
    use windows::Win32::{
        Foundation::{HWND, LPARAM, POINT, RECT},
        Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
            DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, HGDIOBJ, ReleaseDC,
            SRCCOPY, SelectObject,
        },
        System::Threading::GetCurrentProcessId,
        UI::WindowsAndMessaging::{
            EnumWindows, GetClientRect, GetWindowThreadProcessId, IsWindowVisible,
        },
    };

    unsafe extern "system" fn enum_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
        unsafe {
            let windows = &mut *(lparam.0 as *mut Vec<HWND>);
            let mut pid = 0_u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == GetCurrentProcessId() && IsWindowVisible(hwnd).as_bool() {
                let mut rect = RECT::default();
                if GetClientRect(hwnd, &mut rect).is_ok()
                    && rect.right - rect.left > 200
                    && rect.bottom - rect.top > 150
                {
                    windows.push(hwnd);
                }
            }
            BOOL(1)
        }
    }

    unsafe {
        let mut windows = Vec::new();
        EnumWindows(Some(enum_window), LPARAM(&mut windows as *mut _ as isize))
            .map_err(|error| error.to_string())?;
        let hwnd = windows
            .into_iter()
            .max_by_key(|hwnd| {
                let mut rect = RECT::default();
                let _ = GetClientRect(*hwnd, &mut rect);
                (rect.right - rect.left).max(0) * (rect.bottom - rect.top).max(0)
            })
            .ok_or("the app has no visible window to capture")?;

        let mut rect = RECT::default();
        GetClientRect(hwnd, &mut rect).map_err(|error| error.to_string())?;
        let width = (rect.right - rect.left).max(1);
        let height = (rect.bottom - rect.top).max(1);
        let mut origin = POINT::default();
        if !windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut origin).as_bool() {
            return Err("the window's client area could not be located".into());
        }

        let screen = GetDC(None);
        if screen.is_invalid() {
            return Err("the screen could not be captured".into());
        }
        let memory = CreateCompatibleDC(Some(screen));
        if memory.is_invalid() {
            ReleaseDC(None, screen);
            return Err("a capture bitmap could not be made".into());
        }
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        if bitmap.is_invalid() {
            let _ = DeleteDC(memory);
            ReleaseDC(None, screen);
            return Err("a capture bitmap could not be made".into());
        }
        let old = SelectObject(memory, HGDIOBJ::from(bitmap));
        let copied = BitBlt(
            memory,
            0,
            0,
            width,
            height,
            Some(screen),
            origin.x,
            origin.y,
            SRCCOPY,
        );
        SelectObject(memory, old);
        ReleaseDC(None, screen);
        if let Err(error) = copied {
            let _ = DeleteObject(HGDIOBJ::from(bitmap));
            let _ = DeleteDC(memory);
            return Err(error.to_string());
        }

        let mut info = BITMAPINFO::default();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..BITMAPINFOHEADER::default()
        };
        let mut pixels = vec![0_u8; width as usize * height as usize * 4];
        let lines = GetDIBits(
            memory,
            bitmap,
            0,
            height as u32,
            Some(pixels.as_mut_ptr() as *mut c_void),
            &mut info,
            DIB_RGB_COLORS,
        );
        let _ = DeleteObject(HGDIOBJ::from(bitmap));
        let _ = DeleteDC(memory);
        if lines == 0 {
            return Err("the captured pixels could not be read".into());
        }
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.swap(0, 2);
            pixel[3] = 255;
        }
        encode_png(width as u32, height as u32, &pixels)
    }
}

#[cfg(target_os = "windows")]
fn encode_png(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|error| error.to_string())?;
        writer
            .write_image_data(pixels)
            .map_err(|error| error.to_string())?;
        writer.finish().map_err(|error| error.to_string())?;
    }
    Ok(out)
}

#[cfg(target_os = "linux")]
pub(crate) fn window_png() -> Result<Vec<u8>, String> {
    use nsh_platform_linux::gtk4::{
        self, gdk::prelude::*, graphene, gsk::prelude::*, prelude::*,
    };

    let window = gtk4::Window::list_toplevels()
        .into_iter()
        .filter_map(|widget| widget.downcast::<gtk4::Window>().ok())
        .filter(|window| {
            window.is_visible() && window.width() > 100 && window.height() > 100
        })
        .max_by_key(|window| window.width() * window.height())
        .ok_or("the app has no visible window to capture")?;
    let width = window.width() as f32;
    let height = window.height() as f32;
    let paintable = gtk4::WidgetPaintable::new(Some(&window));
    let snapshot = gtk4::Snapshot::new();
    paintable.snapshot(&snapshot, f64::from(width), f64::from(height));
    let node = snapshot
        .to_node()
        .ok_or("the window could not be drawn into a snapshot")?;
    let bounds = graphene::Rect::new(0.0, 0.0, width, height);
    let renderer = window
        .renderer()
        .ok_or("the window has no renderer to read back from")?;
    let texture = renderer.render_texture(&node, Some(&bounds));
    let bytes = texture.save_to_png_bytes();
    let bytes: &[u8] = bytes.as_ref();
    Ok(bytes.to_vec())
}
