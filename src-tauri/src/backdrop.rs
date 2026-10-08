//! What is behind the island: its brightness, for the adaptive keyline (src/lib/island/keyline.ts).
//!
//! The island is a black silhouette. Only against a VERY dark backdrop (a dark wallpaper, a
//! maximised dark title bar) does it need a faint one-pixel keyline to stay readable as a shape;
//! on anything brighter any border would only make it look like a Windows panel. So the frontend
//! needs one number: how bright the pixels just outside the island are.
//!
//! Those pixels are exactly the backdrop: the window region (window.rs) cuts the stage to the island's
//! shape plus a 1 px antialias margin, so everything beyond it belongs to other windows or the
//! wallpaper, never to this one. A few thin strips a few pixels outside the region (below, left and
//! right, and above when the monitor leaves room) are copied from the screen DC with one small
//! `BitBlt` each (a few thousand pixels in all, never the island's own area) and averaged as
//! Rec. 709 luma of the gamma-encoded colour, 0..1: the same scale keyline.ts judges "dark" on.
//!
//! Cost and politeness: one sampling thread, every 2 s while the island is visible (nothing while it
//! is hidden), plus immediately when the display configuration changes, the wallpaper changes or the
//! island settles (`request_sample`, debounced). The value is emitted as `island-backdrop`
//! `{ luminance }` only when it changed. If the screen cannot be read (no DC, BitBlt refused: a
//! secure desktop) the value is null and the frontend draws no keyline.

use crate::monitors::Bounds;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ, SRCCOPY,
};

/// How often the backdrop is looked at while the island is visible.
const INTERVAL: Duration = Duration::from_secs(2);
/// A requested sample waits this long first (a wallpaper fade, a monitor settling).
const DEBOUNCE: Duration = Duration::from_millis(400);
/// Gap between the island's region and the sampled strips, and the strips' thickness (physical px).
const GAP_PX: i32 = 4;
const STRIP_PX: i32 = 2;
/// A change smaller than this is not worth an event.
const EVENT_EPSILON: f64 = 0.01;

static KICK: Mutex<Option<Sender<()>>> = Mutex::new(None);
static LAST: Mutex<Option<f64>> = Mutex::new(None);
static STARTED: OnceLock<()> = OnceLock::new();

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Sample soon (display change, wallpaper change, the island settled). Cheap and safe from any
/// thread; does nothing before `start`.
pub fn request_sample() {
    if let Some(tx) = lock(&KICK).as_ref() {
        let _ = tx.send(());
    }
}

/// The last backdrop brightness, 0 (black) .. 1 (white), or null when it is unknown.
#[tauri::command]
pub fn get_island_backdrop() -> Option<f64> {
    *lock(&LAST)
}

/// The island has settled (or the user asks): look at the backdrop now.
#[tauri::command]
pub fn refresh_island_backdrop() {
    request_sample();
}

/// Strips of backdrop around an island whose region is `island` on a monitor `monitor` (screen px),
/// clipped to the monitor: below, left, right and, when there is room, above. Empty when none fits.
pub fn ring_rects(island: Bounds, monitor: Bounds) -> Vec<Bounds> {
    let outer = GAP_PX + STRIP_PX;
    let candidates = [
        // below
        Bounds { left: island.left, top: island.bottom + GAP_PX, right: island.right, bottom: island.bottom + outer },
        // left
        Bounds { left: island.left - outer, top: island.top, right: island.left - GAP_PX, bottom: island.bottom },
        // right
        Bounds { left: island.right + GAP_PX, top: island.top, right: island.right + outer, bottom: island.bottom },
        // above
        Bounds { left: island.left, top: island.top - outer, right: island.right, bottom: island.top - GAP_PX },
    ];
    candidates
        .into_iter()
        .filter_map(|r| {
            let clipped = Bounds {
                left: r.left.max(monitor.left),
                top: r.top.max(monitor.top),
                right: r.right.min(monitor.right),
                bottom: r.bottom.min(monitor.bottom),
            };
            (clipped.width() > 0 && clipped.height() > 0).then_some(clipped)
        })
        .collect()
}

/// Rec. 709 luma (0..1) of one gamma-encoded pixel, as `keyline.ts` `lumaOf`.
pub(crate) fn luma(r: u8, g: u8, b: u8) -> f64 {
    (0.2126 * r as f64 + 0.7152 * g as f64 + 0.0722 * b as f64) / 255.0
}

/// Sum of luma and pixel count over a top-down 32 bpp BGRA buffer.
pub(crate) fn luma_sum(bgra: &[u8]) -> (f64, usize) {
    let mut sum = 0.0;
    let mut count = 0;
    for px in bgra.chunks_exact(4) {
        sum += luma(px[2], px[1], px[0]);
        count += 1;
    }
    (sum, count)
}

/// Whether a new reading is worth announcing: unknown/known flips, or a real change.
fn differs(old: Option<f64>, new: Option<f64>) -> bool {
    match (old, new) {
        (None, None) => false,
        (Some(a), Some(b)) => (a - b).abs() >= EVENT_EPSILON,
        _ => true,
    }
}

/// Mean luma of the screen inside `rects`, or None if nothing could be read.
fn sample_screen(rects: &[Bounds]) -> Option<f64> {
    if rects.is_empty() {
        return None;
    }
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return None;
        }
        let memory = CreateCompatibleDC(screen);
        let mut sum = 0.0;
        let mut count = 0usize;
        if !memory.is_invalid() {
            for r in rects {
                let (w, h) = (r.width(), r.height());
                let bitmap = CreateCompatibleBitmap(screen, w, h);
                if bitmap.is_invalid() {
                    continue;
                }
                let previous = SelectObject(memory, HGDIOBJ(bitmap.0));
                if BitBlt(memory, 0, 0, w, h, screen, r.left, r.top, SRCCOPY).is_ok() {
                    let mut info = BITMAPINFO {
                        bmiHeader: BITMAPINFOHEADER {
                            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                            biWidth: w,
                            biHeight: -h, // top-down
                            biPlanes: 1,
                            biBitCount: 32,
                            biCompression: BI_RGB.0,
                            ..Default::default()
                        },
                        ..Default::default()
                    };
                    let mut pixels = vec![0u8; (w as usize) * (h as usize) * 4];
                    // Deselect before GetDIBits (the bitmap must not be selected into a DC).
                    SelectObject(memory, previous);
                    let rows = GetDIBits(memory, bitmap, 0, h as u32, Some(pixels.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
                    if rows > 0 {
                        let (s, c) = luma_sum(&pixels);
                        sum += s;
                        count += c;
                    }
                } else {
                    SelectObject(memory, previous);
                }
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
            }
            let _ = DeleteDC(memory);
        }
        ReleaseDC(None, screen);
        (count > 0).then(|| sum / count as f64)
    }
}

fn sample_once(app: &AppHandle) {
    if !crate::window::is_visible(app) {
        return;
    }
    let reading = crate::window::island_screen().and_then(|(island, monitor)| sample_screen(&ring_rects(island, monitor)));
    let changed = {
        let mut last = lock(&LAST);
        let changed = differs(*last, reading);
        if changed {
            *last = reading;
        }
        changed
    };
    if changed {
        if let Err(e) = app.emit("island-backdrop", serde_json::json!({ "luminance": reading })) {
            dlog!("WARN", "backdrop", "emit island-backdrop failed: {}", e);
        }
    }
}

/// Start the sampling thread (once).
pub fn start(app: AppHandle) {
    if STARTED.set(()).is_err() {
        return;
    }
    let (tx, rx) = channel::<()>();
    *lock(&KICK) = Some(tx);
    let spawned = std::thread::Builder::new().name("island-backdrop".into()).spawn(move || loop {
        let kicked = match rx.recv_timeout(INTERVAL) {
            Ok(()) => true,
            Err(RecvTimeoutError::Timeout) => false,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        if kicked {
            // Let the change settle, and fold the requests that piled up meanwhile into this one.
            std::thread::sleep(DEBOUNCE);
            while rx.try_recv().is_ok() {}
        }
        crate::debug_log::catch("backdrop", || sample_once(&app));
    });
    if let Err(e) = spawned {
        dlog!("WARN", "backdrop", "sampling thread did not start: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONITOR: Bounds = Bounds { left: 0, top: 0, right: 1920, bottom: 1080 };

    fn island() -> Bounds {
        // 200x44 island at 100%: region 1 px wider than the island, 7 px from the top.
        Bounds { left: 859, top: 7, right: 1061, bottom: 53 }
    }

    #[test]
    fn strips_sit_outside_the_region_never_inside() {
        let island = island();
        for r in ring_rects(island, MONITOR) {
            let overlaps = r.left < island.right && r.right > island.left && r.top < island.bottom && r.bottom > island.top;
            assert!(!overlaps, "{r:?} overlaps the island region");
            // At least GAP_PX from the region on the side it lies on.
            let gap = (island.left - r.right).max(r.left - island.right).max(r.top - island.bottom).max(island.top - r.bottom);
            assert!(gap >= GAP_PX, "{r:?} is only {gap} px away");
        }
    }

    #[test]
    fn a_free_island_gets_all_four_strips_and_a_screen_top_island_drops_the_one_above() {
        assert_eq!(ring_rects(island(), MONITOR).len(), 4, "7 px below the edge: the strip 1..3 px fits above");
        let lower = Bounds { top: 20, bottom: 66, ..island() };
        assert_eq!(ring_rects(lower, MONITOR).len(), 4);
        let flush = Bounds { top: 0, bottom: 46, ..island() };
        assert_eq!(ring_rects(flush, MONITOR).len(), 3);
    }

    #[test]
    fn strips_are_clipped_to_the_monitor() {
        let corner = Bounds { left: 0, top: 7, right: 200, bottom: 53 };
        for r in ring_rects(corner, MONITOR) {
            assert!(r.left >= MONITOR.left && r.right <= MONITOR.right && r.top >= MONITOR.top && r.bottom <= MONITOR.bottom);
            assert!(r.width() > 0 && r.height() > 0);
        }
        // Nothing fits on a monitor the island fills.
        assert!(ring_rects(MONITOR, MONITOR).is_empty());
    }

    #[test]
    fn luma_matches_the_frontend_scale() {
        assert_eq!(luma(0, 0, 0), 0.0);
        assert!((luma(255, 255, 255) - 1.0).abs() < 1e-9);
        // navy (0,0,128) and near black are dark enough for a keyline, mid grey is not (< 0.12).
        assert!(luma(0, 0, 128) < 0.12);
        assert!(luma(10, 10, 10) < 0.12);
        assert!(luma(128, 128, 128) > 0.18);
    }

    #[test]
    fn luma_sum_reads_bgra() {
        // One white and one black pixel (B, G, R, A).
        let (sum, count) = luma_sum(&[255, 255, 255, 255, 0, 0, 0, 255]);
        assert_eq!(count, 2);
        assert!((sum / count as f64 - 0.5).abs() < 1e-9);
        // A pure red pixel weighs by the red coefficient only.
        let (red, _) = luma_sum(&[0, 0, 255, 255]);
        assert!((red - 0.2126).abs() < 1e-9);
    }

    #[test]
    fn events_are_sent_for_real_changes_only() {
        assert!(!differs(None, None));
        assert!(differs(None, Some(0.2)));
        assert!(differs(Some(0.2), None));
        assert!(!differs(Some(0.20), Some(0.205)));
        assert!(differs(Some(0.20), Some(0.25)));
    }
}
