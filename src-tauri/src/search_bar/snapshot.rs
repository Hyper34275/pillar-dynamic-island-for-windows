//! The frosted-glass backdrop of the centre search bar ("Spotlight glass", docs/DESIGN_SYSTEM.md).
//!
//! WebView2 cannot blur what lies behind its own transparent window, so the bar takes a picture of the
//! screen under the window's rectangle BEFORE it shows (the window is hidden meanwhile, so the picture
//! holds only what the user sees), shrinks it 4x with a halftone `StretchBlt` (a few hundred pixels
//! across: the page blurs it further and scales it back up) and hands it to the page as a small BMP
//! data URL, together with its mean brightness (the page picks the tint from that) and the two Windows
//! personalisation flags the page cannot read itself: the app theme and "transparency effects".
//!
//! Privacy: the picture is pixels of the user's screen. It lives in memory only (this module's slot in
//! `glass.rs`, the event payload, the page) and is dropped when the bar closes. It is never written to
//! disk and never logged; only its size and the time it took are.
//!
//! Failure is ordinary, not an error: no screen DC, a refused `StretchBlt` (secure desktop), an all
//! black picture (protected content, a locked session), transparency effects off or high contrast.
//! Then the backdrop carries no image and the page paints its near-opaque fallback tint.

use crate::monitors::Bounds;
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SelectObject, SetBrushOrgEx,
    SetStretchBltMode, StretchBlt, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HALFTONE, HGDIOBJ, SRCCOPY,
};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

/// The picture is this many times smaller than the window in physical pixels, on both axes.
pub const DOWNSCALE: i32 = 4;
/// A window larger than this many pixels on a side is not captured (a bad measurement).
const MAX_SIDE_PX: i32 = 8192;

const PERSONALIZE_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";

/// What the page gets (event `search-bar-backdrop`, command `search_bar_backdrop`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlassBackdrop {
    /// Counts up per capture, so the page can tell a new picture from the one it already holds.
    pub id: u64,
    /// `data:image/bmp;base64,...` of the screen under the window, or null (fallback tint).
    pub image: Option<String>,
    /// Mean Rec. 709 luma of the picture, 0 (black) .. 1 (white); null without a picture.
    pub luminance: Option<f64>,
    /// The Windows app theme is dark.
    pub dark: bool,
    /// "Transparency effects" are on in Windows.
    pub transparency: bool,
}

/// A downscaled picture: top-down 32 bpp BGRA.
#[derive(Debug, PartialEq)]
pub struct Shot {
    pub width: i32,
    pub height: i32,
    pub bgra: Vec<u8>,
}

// =============================================================================
// Pure parts
// =============================================================================

/// Size of the picture for a window of `w` x `h` physical px: every side divided by `DOWNSCALE`,
/// rounded up so a window of one pixel still has one. `None` for an empty or absurd window.
pub fn small_size(w: i32, h: i32) -> Option<(i32, i32)> {
    if w <= 0 || h <= 0 || w > MAX_SIDE_PX || h > MAX_SIDE_PX {
        return None;
    }
    Some(((w + DOWNSCALE - 1) / DOWNSCALE, (h + DOWNSCALE - 1) / DOWNSCALE))
}

/// Windows "app mode" is dark when `AppsUseLightTheme` is 0; a missing value is the light default.
pub fn theme_is_dark(apps_use_light: Option<u32>) -> bool {
    apps_use_light == Some(0)
}

/// "Transparency effects": only an explicit 0 turns them off.
pub fn transparency_on(enable_transparency: Option<u32>) -> bool {
    enable_transparency != Some(0)
}

/// Whether a picture is worth taking at all: not with transparency effects off (the page draws an
/// almost opaque panel, so the picture would be wasted) and not in high contrast (plain colours).
pub fn wants_capture(transparency: bool, high_contrast: bool) -> bool {
    transparency && !high_contrast
}

/// A picture with every colour channel zero: what a secure desktop or protected content gives.
pub fn is_blank(bgra: &[u8]) -> bool {
    bgra.chunks_exact(4).all(|p| p[0] == 0 && p[1] == 0 && p[2] == 0)
}

/// Mean Rec. 709 luma (0..1) of a BGRA buffer, the scale `keyline.ts` and `backdrop.rs` use.
pub fn mean_luminance(bgra: &[u8]) -> Option<f64> {
    let (sum, count) = crate::backdrop::luma_sum(bgra);
    (count > 0).then(|| sum / count as f64)
}

/// An uncompressed 24 bpp bottom-up BMP of a top-down BGRA buffer (the alpha channel is dropped).
/// Rows are padded to four bytes as the format requires. `None` when the buffer does not match.
pub fn encode_bmp24(width: i32, height: i32, bgra: &[u8]) -> Option<Vec<u8>> {
    if width <= 0 || height <= 0 || bgra.len() != (width as usize) * (height as usize) * 4 {
        return None;
    }
    let (w, h) = (width as usize, height as usize);
    let row = (w * 3).div_ceil(4) * 4;
    let image = row * h;
    let total = 54 + image;
    let mut out = Vec::with_capacity(total);
    // BITMAPFILEHEADER
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    // BITMAPINFOHEADER
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes()); // positive: rows run bottom to top
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&(image as u32).to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for y in (0..h).rev() {
        let line = &bgra[y * w * 4..(y + 1) * w * 4];
        for px in line.chunks_exact(4) {
            out.extend_from_slice(&px[..3]); // B, G, R
        }
        out.resize(out.len() + (row - w * 3), 0);
    }
    Some(out)
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding (no crate for the one place that needs it).
pub fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

/// The backdrop for one open: the picture (when there is a usable one) as a data URL with its mean
/// brightness, always with the two Windows flags. No picture, or an all black one, gives no image.
pub fn assemble(id: u64, dark: bool, transparency: bool, shot: Option<&Shot>) -> GlassBackdrop {
    let usable = shot.filter(|s| !is_blank(&s.bgra));
    let image = usable.and_then(|s| encode_bmp24(s.width, s.height, &s.bgra)).map(|bmp| format!("data:image/bmp;base64,{}", base64(&bmp)));
    let luminance = if image.is_some() { usable.and_then(|s| mean_luminance(&s.bgra)) } else { None };
    GlassBackdrop { id, image, luminance, dark, transparency }
}

// =============================================================================
// Win32
// =============================================================================

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn read_dword(value: &str) -> Option<u32> {
    let key = wide(PERSONALIZE_KEY);
    let name = wide(value);
    let mut data = 0u32;
    let mut size = 4u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    status.is_ok().then_some(data)
}

/// `(dark, transparency)` from the Windows personalisation settings.
pub fn read_personalization() -> (bool, bool) {
    (theme_is_dark(read_dword("AppsUseLightTheme")), transparency_on(read_dword("EnableTransparency")))
}

/// The screen under `rect` (physical px, virtual-screen coordinates; the process is per-monitor DPI
/// aware) shrunk `DOWNSCALE` times with a halftone stretch. `None` when the screen cannot be read.
fn grab(rect: Bounds) -> Option<Shot> {
    let (w, h) = (rect.width(), rect.height());
    let (dw, dh) = small_size(w, h)?;
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return None;
        }
        let memory = CreateCompatibleDC(screen);
        let mut shot = None;
        if !memory.is_invalid() {
            let bitmap = CreateCompatibleBitmap(screen, dw, dh);
            if !bitmap.is_invalid() {
                let previous = SelectObject(memory, HGDIOBJ(bitmap.0));
                SetStretchBltMode(memory, HALFTONE);
                let _ = SetBrushOrgEx(memory, 0, 0, None);
                let stretched = StretchBlt(memory, 0, 0, dw, dh, screen, rect.left, rect.top, w, h, SRCCOPY).as_bool();
                // The bitmap must not be selected into a DC while GetDIBits reads it.
                SelectObject(memory, previous);
                if stretched {
                    let mut info = BITMAPINFO {
                        bmiHeader: BITMAPINFOHEADER {
                            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                            biWidth: dw,
                            biHeight: -dh, // top-down
                            biPlanes: 1,
                            biBitCount: 32,
                            biCompression: BI_RGB.0,
                            ..Default::default()
                        },
                        ..Default::default()
                    };
                    let mut bgra = vec![0u8; (dw as usize) * (dh as usize) * 4];
                    let rows = GetDIBits(memory, bitmap, 0, dh as u32, Some(bgra.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
                    if rows > 0 {
                        shot = Some(Shot { width: dw, height: dh, bgra });
                    }
                }
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
            }
            let _ = DeleteDC(memory);
        }
        ReleaseDC(None, screen);
        shot
    }
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Take the backdrop of the glass window whose rectangle is `window` (physical px). The window must
/// be hidden. `high_contrast` skips the picture; so does a Windows with transparency effects off.
pub fn capture(window: Bounds, high_contrast: bool) -> GlassBackdrop {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let (dark, transparency) = read_personalization();
    if !wants_capture(transparency, high_contrast) {
        return assemble(id, dark, transparency, None);
    }
    let started = Instant::now();
    let shot = grab(window);
    let backdrop = assemble(id, dark, transparency, shot.as_ref());
    // Size and time only: never a pixel of it.
    dlog!(
        "INFO",
        "search_bar",
        "glass backdrop {}: {}x{} px grabbed in {} ms",
        if backdrop.image.is_some() { "ok" } else { "unavailable" },
        shot.as_ref().map_or(0, |s| s.width),
        shot.as_ref().map_or(0, |s| s.height),
        started.elapsed().as_millis()
    );
    backdrop
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(b: u8, g: u8, r: u8) -> [u8; 4] {
        [b, g, r, 255]
    }

    fn buffer(pixels: &[[u8; 4]]) -> Vec<u8> {
        pixels.iter().flatten().copied().collect()
    }

    #[test]
    fn the_picture_is_a_quarter_of_the_window_rounded_up_at_every_dpi() {
        // the window is 760 x 640 DIP: 100%, 125%, 150%, 200% in physical px
        for (scale, w, h) in [(1.0, 760, 640), (1.25, 950, 800), (1.5, 1140, 960), (2.0, 1520, 1280)] {
            assert_eq!(((760.0 * scale) as i32, (640.0 * scale) as i32), (w, h));
            let (dw, dh) = small_size(w, h).unwrap();
            assert_eq!((dw, dh), ((w + 3) / 4, (h + 3) / 4));
            // the whole source maps onto the whole picture: the page stretches it back over the window
            assert!(dw * DOWNSCALE >= w && (dw - 1) * DOWNSCALE < w, "{w}: {dw}");
            assert!(dh * DOWNSCALE >= h && (dh - 1) * DOWNSCALE < h, "{h}: {dh}");
        }
        // 175% gives a window that is not a multiple of four
        assert_eq!(small_size(1330, 1120), Some((333, 280)));
        assert_eq!(small_size(1, 1), Some((1, 1)));
    }

    #[test]
    fn an_empty_or_absurd_window_is_not_captured() {
        assert_eq!(small_size(0, 100), None);
        assert_eq!(small_size(100, -1), None);
        assert_eq!(small_size(MAX_SIDE_PX + 1, 100), None);
        assert!(small_size(MAX_SIDE_PX, MAX_SIDE_PX).is_some());
    }

    #[test]
    fn luminance_is_the_keyline_scale() {
        let black = buffer(&[px(0, 0, 0), px(0, 0, 0)]);
        let white = buffer(&[px(255, 255, 255), px(255, 255, 255)]);
        let half = buffer(&[px(0, 0, 0), px(255, 255, 255)]);
        assert_eq!(mean_luminance(&black), Some(0.0));
        assert!((mean_luminance(&white).unwrap() - 1.0).abs() < 1e-9);
        assert!((mean_luminance(&half).unwrap() - 0.5).abs() < 1e-9);
        // BGRA order: a pure red pixel weighs by the red coefficient only
        assert!((mean_luminance(&buffer(&[px(0, 0, 255)])).unwrap() - 0.2126).abs() < 1e-9);
        assert_eq!(mean_luminance(&[]), None);
    }

    #[test]
    fn blank_pictures_are_recognised() {
        assert!(is_blank(&buffer(&[px(0, 0, 0), px(0, 0, 0)])));
        assert!(is_blank(&[]), "nothing to show is blank");
        assert!(!is_blank(&buffer(&[px(0, 0, 0), px(0, 1, 0)])));
        // alpha does not count: GDI leaves it zero on a real picture too
        assert!(!is_blank(&[0, 0, 7, 0]));
    }

    #[test]
    fn bmp_header_and_pixel_order() {
        // 2 x 2: top row red, green; bottom row blue, white
        let bgra = buffer(&[px(0, 0, 255), px(0, 255, 0), px(255, 0, 0), px(255, 255, 255)]);
        let bmp = encode_bmp24(2, 2, &bgra).unwrap();
        assert_eq!(&bmp[..2], b"BM");
        let u32_at = |o: usize| u32::from_le_bytes(bmp[o..o + 4].try_into().unwrap());
        let i32_at = |o: usize| i32::from_le_bytes(bmp[o..o + 4].try_into().unwrap());
        assert_eq!(u32_at(2) as usize, bmp.len(), "file size");
        assert_eq!(u32_at(10), 54, "pixel data offset");
        assert_eq!(u32_at(14), 40, "info header size");
        assert_eq!((i32_at(18), i32_at(22)), (2, 2));
        assert_eq!(u16::from_le_bytes([bmp[26], bmp[27]]), 1, "planes");
        assert_eq!(u16::from_le_bytes([bmp[28], bmp[29]]), 24, "bits per pixel");
        assert_eq!(u32_at(30), 0, "uncompressed");
        // each row is 2 px * 3 = 6 bytes, padded to 8; two rows
        assert_eq!(u32_at(34), 16);
        assert_eq!(bmp.len(), 54 + 16);
        // bottom-up: the first row in the file is the bottom row (blue, white), as B G R
        assert_eq!(&bmp[54..60], &[255, 0, 0, 255, 255, 255]);
        assert_eq!(&bmp[60..62], &[0, 0], "row padding");
        // then the top row (red, green)
        assert_eq!(&bmp[62..68], &[0, 0, 255, 0, 255, 0]);
    }

    #[test]
    fn bmp_rows_are_padded_to_four_bytes() {
        for w in 1..=9usize {
            let bgra = vec![200u8; w * 3 * 4];
            let bmp = encode_bmp24(w as i32, 3, &bgra).unwrap();
            let row = (w * 3).div_ceil(4) * 4;
            assert_eq!(row % 4, 0);
            assert_eq!(bmp.len(), 54 + row * 3, "width {w}");
        }
    }

    #[test]
    fn bmp_refuses_a_buffer_of_the_wrong_size() {
        assert!(encode_bmp24(2, 2, &[0u8; 15]).is_none());
        assert!(encode_bmp24(0, 2, &[]).is_none());
        assert!(encode_bmp24(2, -1, &[0u8; 8]).is_none());
    }

    #[test]
    fn base64_matches_the_rfc_vectors() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), expected, "{input:?}");
        }
        // every byte value survives (the alphabet's last two characters are used)
        assert_eq!(base64(&[0xFB, 0xFF, 0xBF]), "+/+/");
    }

    #[test]
    fn a_picture_becomes_a_data_url_with_its_brightness() {
        let shot = Shot { width: 2, height: 1, bgra: buffer(&[px(0, 0, 0), px(255, 255, 255)]) };
        let b = assemble(7, true, true, Some(&shot));
        assert_eq!(b.id, 7);
        assert!(b.dark && b.transparency);
        let url = b.image.expect("a picture");
        assert!(url.starts_with("data:image/bmp;base64,Qk"), "BM in base64: {}", &url[..30]);
        assert!((b.luminance.unwrap() - 0.5).abs() < 1e-9);
        // the same picture twice gives the same bytes: nothing time or address dependent in it
        assert_eq!(assemble(8, true, true, Some(&shot)).image.as_deref(), Some(url.as_str()));
    }

    #[test]
    fn no_picture_a_blank_picture_or_a_bad_buffer_all_fall_back_to_the_tint() {
        let blank = Shot { width: 2, height: 1, bgra: buffer(&[px(0, 0, 0), px(0, 0, 0)]) };
        let bad = Shot { width: 4, height: 4, bgra: vec![9; 3] };
        for shot in [None, Some(&blank), Some(&bad)] {
            let b = assemble(1, false, true, shot);
            assert_eq!((b.image, b.luminance), (None, None));
            assert!(!b.dark && b.transparency, "the Windows flags still arrive");
        }
    }

    #[test]
    fn the_windows_settings_decide_theme_transparency_and_whether_to_capture() {
        assert!(theme_is_dark(Some(0)));
        assert!(!theme_is_dark(Some(1)));
        assert!(!theme_is_dark(None), "no value: Windows' light default");
        assert!(transparency_on(Some(1)));
        assert!(transparency_on(None), "no value: effects are on by default");
        assert!(!transparency_on(Some(0)));
        assert!(wants_capture(true, false));
        assert!(!wants_capture(false, false), "transparency off: no picture");
        assert!(!wants_capture(true, true), "high contrast: no picture");
    }

    #[test]
    fn the_payload_is_camel_case_for_the_page() {
        let b = assemble(3, true, false, None);
        let json = serde_json::to_value(&b).unwrap();
        let mut keys: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, vec!["dark", "id", "image", "luminance", "transparency"]);
        assert!(json["image"].is_null() && json["luminance"].is_null());
    }

    /// Writes a synthetic picture (red at the top fading to blue at the bottom, a green block in the
    /// top-right corner) as a BMP to the file named by `GLASS_BMP_OUT`, for a check in a real browser
    /// that the bytes decode the right way up and in the right channel order. No screen content.
    #[test]
    #[ignore]
    fn writes_a_synthetic_bmp_for_a_browser_check() {
        let Ok(path) = std::env::var("GLASS_BMP_OUT") else { return };
        let (w, h) = (190i32, 160i32);
        let mut bgra = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let t = y as f64 / (h - 1) as f64;
                let (mut b, g, mut r) = ((255.0 * t) as u8, 0u8, (255.0 * (1.0 - t)) as u8);
                let mut green = g;
                if x >= w - 40 && y < 40 {
                    (b, green, r) = (0, 255, 0);
                }
                bgra.extend_from_slice(&[b, green, r, 255]);
            }
        }
        let bmp = encode_bmp24(w, h, &bgra).unwrap();
        std::fs::write(path, bmp).unwrap();
    }

    /// A whole glass window's capture (760 x 640 DIP at 100%) on the real screen: it works, it is quick
    /// (the page waits for it before the bar shows) and the data URL is small. Ignored like the one
    /// below: it needs a desktop. `--nocapture` prints the numbers.
    #[test]
    #[ignore]
    fn a_full_glass_capture_on_the_real_screen_is_quick_and_small() {
        let window = Bounds { left: 100, top: 100, right: 860, bottom: 740 };
        let started = Instant::now();
        let b = capture(window, false);
        let took = started.elapsed();
        let size = b.image.as_ref().map_or(0, |s| s.len());
        eprintln!("glass capture: {} ms, data URL {} bytes, luminance {:?}, dark {}, transparency {}", took.as_millis(), size, b.luminance, b.dark, b.transparency);
        assert!(took.as_millis() < 250, "{took:?}");
        if let Some(url) = &b.image {
            assert!(url.starts_with("data:image/bmp;base64,"));
            assert!(size < 400_000, "{size}");
            assert!(b.luminance.is_some());
        }
    }

    /// The real screen: a small capture of the top-left of the desktop. Ignored by default (a CI
    /// session has no desktop); run with `--ignored` on a machine with a screen.
    #[test]
    #[ignore]
    fn grabbing_the_real_screen_gives_a_picture_of_the_right_size() {
        let shot = grab(Bounds { left: 0, top: 0, right: 800, bottom: 600 }).expect("a screen");
        assert_eq!((shot.width, shot.height), (200, 150));
        assert_eq!(shot.bgra.len(), 200 * 150 * 4);
    }
}
