//! Native Wayland screen capture (B3 / T10-Phase2).
//!
//! xcap cannot read pixels on compositors that expose no X11 server (and on
//! Wayland-only setups the security model forbids ambient screen reads
//! anyway). This module provides two native backends, tried in order by
//! [`capture_screen_wayland`]:
//!
//! 1. **wlr-screencopy** (`zwlr_screencopy_manager_v1`) — direct compositor
//!    copy into a `wl_shm` buffer. Works on sway, Hyprland, river, wayfire
//!    and other wlroots-family compositors. No dialogs, no portal.
//! 2. **xdg-desktop-portal Screenshot** — the cross-desktop fallback
//!    (GNOME, KDE). The desktop may show a one-time permission prompt and
//!    returns the screenshot as a file URI, which we decode with `image`.
//!
//! If both fail, the caller falls back to xcap (useful under XWayland).
//! Compositor-by-compositor verification matrix lives in
//! `docs/qa/2026-09-07-computer-use-browser-qa-checklist.md` (QA-5).
//!
//! v1 scope: capture the first enumerated `wl_output` (mirrors the xcap
//! path's "first monitor" behavior). Multi-head targeting is tracked in
//! the followups roadmap.

use image::DynamicImage;

/// True when the session looks Wayland-native (`WAYLAND_DISPLAY` set).
pub fn wayland_session_active() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
}

/// Capture via wlr-screencopy, then the portal. Both failures are chained
/// into the error so the caller can surface one actionable message.
pub async fn capture_screen_wayland() -> Result<DynamicImage, String> {
    let wlr = capture_first_output().await;
    match wlr {
        Ok(img) => Ok(img),
        Err(wlr_err) => {
            tracing::debug!(error = %wlr_err, "wlr-screencopy unavailable, trying portal");
            match capture_via_portal().await {
                Ok(img) => Ok(img),
                Err(portal_err) => Err(format!(
                    "Wayland capture failed on both backends.\n  wlr-screencopy: {wlr_err}\n  portal: {portal_err}"
                )),
            }
        }
    }
}

/// Capture the first `wl_output` via `zwlr_screencopy_manager_v1`.
///
/// The whole Wayland exchange runs on a blocking thread: the event queue
/// drives with `blocking_dispatch`, which must never run on an async
/// executor worker.
pub async fn capture_first_output() -> Result<DynamicImage, String> {
    tokio::task::spawn_blocking(capture_first_output_blocking)
        .await
        .map_err(|e| format!("capture thread join: {e}"))?
}

/// One screenshot through the xdg-desktop-portal Screenshot interface.
/// The portal writes the pixels to a cache file which we decode and then
/// delete (it may contain sensitive content).
pub async fn capture_via_portal() -> Result<DynamicImage, String> {
    let request = ashpd::desktop::screenshot::Screenshot::request().interactive(false);
    let response = request
        .send()
        .await
        .map_err(|e| format!("portal screenshot request: {e}"))?;
    let shot = response
        .response()
        .map_err(|e| format!("portal screenshot response: {e}"))?;
    let path = shot
        .uri()
        .to_file_path()
        .map_err(|_| format!("portal returned a non-file URI: {}", shot.uri()))?;
    let decoded = image::ImageReader::open(&path)
        .and_then(|r| r.with_guessed_format())
        .map_err(|e| format!("open portal screenshot {}: {e}", path.display()))?
        .decode()
        .map_err(|e| format!("decode portal screenshot {}: {e}", path.display()));
    // Best-effort cleanup regardless of decode outcome.
    let _ = std::fs::remove_file(&path);
    decoded
}

/// Convert an Argb8888/Xrgb8888 (little-endian BGRA in memory) shm buffer
/// with the given row stride into tightly-packed RGBA bytes.
fn shm_to_rgba(buf: &[u8], width: u32, height: u32, stride: usize) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let mut out = Vec::with_capacity(w * h * 4);
    for row in 0..h {
        let row_start = row * stride;
        for px in 0..w {
            let i = row_start + px * 4;
            if i + 3 >= buf.len() {
                // Padding beyond the buffer would be a compositor bug; emit
                // opaque black rather than panicking.
                out.extend_from_slice(&[0, 0, 0, 255]);
                continue;
            }
            let b = buf[i];
            let g = buf[i + 1];
            let r = buf[i + 2];
            // Screencopy never promises meaningful alpha; treat 0 as opaque
            // so the PNG does not come out transparent.
            let a = if buf[i + 3] == 0 { 255 } else { buf[i + 3] };
            out.extend_from_slice(&[r, g, b, a]);
        }
    }
    out
}

fn capture_first_output_blocking() -> Result<DynamicImage, String> {
    use wayland_client::globals::{GlobalListContents, registry_queue_init};
    use wayland_client::protocol::{
        wl_buffer::WlBuffer, wl_output::WlOutput, wl_registry, wl_registry::WlRegistry, wl_shm,
        wl_shm::WlShm, wl_shm_pool::WlShmPool,
    };
    use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
    use wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_frame_v1::{
        self, ZwlrScreencopyFrameV1,
    };
    use wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1;

    #[derive(Default)]
    struct State {
        outputs: Vec<WlOutput>,
        /// (format, width, height, stride) announced by the capture frame.
        buffer_details: Option<(wl_shm::Format, i32, i32, i32)>,
        status: FrameStatus,
    }

    #[derive(Default, PartialEq, Eq, Clone, Copy, Debug)]
    enum FrameStatus {
        #[default]
        Waiting,
        Ready,
        Failed,
    }

    impl Dispatch<WlRegistry, GlobalListContents> for State {
        fn event(
            state: &mut Self,
            registry: &WlRegistry,
            event: wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            qh: &QueueHandle<Self>,
        ) {
            if let wl_registry::Event::Global {
                name,
                interface,
                version,
            } = event
            {
                if interface == WlOutput::interface().name {
                    // v2 is plenty (screencopy only needs the object); newer
                    // wl_output events are additive.
                    let cap = version.min(2);
                    state.outputs.push(registry.bind(name, cap, qh, ()));
                }
            }
        }
    }

    wayland_client::delegate_noop!(State: ignore WlOutput);
    wayland_client::delegate_noop!(State: ignore WlShm);
    wayland_client::delegate_noop!(State: ignore WlShmPool);
    wayland_client::delegate_noop!(State: ignore WlBuffer);
    wayland_client::delegate_noop!(State: ignore ZwlrScreencopyManagerV1);

    impl Dispatch<ZwlrScreencopyFrameV1, ()> for State {
        fn event(
            state: &mut Self,
            _: &ZwlrScreencopyFrameV1,
            event: zwlr_screencopy_frame_v1::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            match event {
                zwlr_screencopy_frame_v1::Event::Buffer {
                    format,
                    width,
                    height,
                    stride,
                } => match format {
                    wayland_client::WEnum::Value(f) => {
                        state.buffer_details =
                            Some((f, width as i32, height as i32, stride as i32));
                    }
                    wayland_client::WEnum::Unknown(v) => {
                        state.status = FrameStatus::Failed;
                        tracing::warn!(format = v, "compositor announced unknown shm format");
                    }
                },
                zwlr_screencopy_frame_v1::Event::Flags { .. } => {}
                zwlr_screencopy_frame_v1::Event::Ready { .. } => state.status = FrameStatus::Ready,
                zwlr_screencopy_frame_v1::Event::Failed => state.status = FrameStatus::Failed,
                _ => {}
            }
        }
    }

    let conn = Connection::connect_to_env().map_err(|e| format!("wayland connect: {e}"))?;
    let (globals, mut queue) =
        registry_queue_init::<State>(&conn).map_err(|e| format!("wayland registry: {e}"))?;
    let qh = queue.handle();

    let mut state = State::default();

    let shm: WlShm = globals
        .bind(&qh, 1..=1, ())
        .map_err(|e| format!("bind wl_shm: {e:?}"))?;
    let manager: ZwlrScreencopyManagerV1 = globals.bind(&qh, 1..=3, ()).map_err(|e| {
        format!(
            "bind zwlr_screencopy_manager_v1 (compositor does not advertise it; \
                 use the portal path or a wlroots compositor): {e:?}"
        )
    })?;

    // Initial registry events (including wl_output globals) were already
    // dispatched by registry_queue_init into the same State.
    let output = state
        .outputs
        .first()
        .cloned()
        .ok_or_else(|| "no wl_output advertised by the compositor".to_string())?;

    let frame = manager.capture_output(0, &output, &qh, ());

    // Wait for the frame's Buffer event to learn format/geometry.
    queue
        .roundtrip(&mut state)
        .map_err(|e| format!("wayland roundtrip: {e}"))?;
    let (format, width, height, stride) = state
        .buffer_details
        .ok_or_else(|| "compositor sent no buffer details for the capture frame".to_string())?;
    if !(format == wl_shm::Format::Argb8888 || format == wl_shm::Format::Xrgb8888) {
        return Err(format!("unsupported shm format {format:?}"));
    }
    if width <= 0 || height <= 0 || stride < width * 4 {
        return Err(format!(
            "implausible frame geometry {width}x{height} stride {stride}"
        ));
    }

    // SHM backing file: create, size, mmap, then hand the fd to the pool.
    // Unlinking the name immediately keeps the pixels off the filesystem;
    // the mapping and the fd carried by the pool outlive the path.
    let size = (stride as usize) * (height as usize);
    let path = std::env::temp_dir().join(format!(
        "shannon-screencopy-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| format!("create shm file {}: {e}", path.display()))?;
    let _ = std::fs::remove_file(&path);
    file.set_len(size as u64)
        .map_err(|e| format!("size shm file: {e}"))?;
    let mmap = unsafe { memmap2::MmapOptions::new().map_mut(&file) }
        .map_err(|e| format!("mmap shm: {e}"))?;

    use std::os::fd::AsFd;
    let pool: WlShmPool = shm.create_pool(file.as_fd(), size as i32, &qh, ());
    let _buffer: WlBuffer = pool.create_buffer(0, width, height, stride, format, &qh, ());

    frame.copy(&_buffer);

    // Drive the queue until the copy completes. Compositors force a repaint
    // for screencopy, so this converges within a few frames; the bound
    // guards against a wedged compositor.
    for _ in 0..300 {
        if state.status != FrameStatus::Waiting {
            break;
        }
        queue
            .blocking_dispatch(&mut state)
            .map_err(|e| format!("wayland dispatch: {e}"))?;
    }
    match state.status {
        FrameStatus::Ready => {}
        FrameStatus::Failed => return Err("compositor refused the capture (frame failed)".into()),
        FrameStatus::Waiting => {
            return Err("compositor did not complete the capture in time".into());
        }
    }

    let rgba = shm_to_rgba(&mmap, width as u32, height as u32, stride as usize);
    image::RgbaImage::from_raw(width as u32, height as u32, rgba)
        .map(DynamicImage::ImageRgba8)
        .ok_or_else(|| "capture buffer size mismatch".to_string())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::shm_to_rgba;

    #[test]
    fn converts_bgra_with_stride_to_rgba() {
        // 2x2 image, stride 12 (4 bytes of padding per row).
        // Pixel bytes: B G R A in memory.
        let buf = [
            1u8, 2, 3, 0, 4, 5, 6, 0, 9, 9, 9, 9, // row 0 + pad
            7, 8, 9, 0, 10, 11, 12, 0, 9, 9, 9, 9, // row 1 + pad
        ];
        let out = shm_to_rgba(&buf, 2, 2, 12);
        assert_eq!(out.len(), 16);
        // First pixel: R=3 G=2 B=1, alpha forced opaque.
        assert_eq!(&out[0..4], &[3, 2, 1, 255]);
        assert_eq!(&out[4..8], &[6, 5, 4, 255]);
        // Row 1 starts after the stride.
        assert_eq!(&out[8..12], &[9, 8, 7, 255]);
        assert_eq!(&out[12..16], &[12, 11, 10, 255]);
    }

    #[test]
    fn short_buffer_yields_opaque_black_not_panic() {
        let buf = [1u8, 2, 3];
        let out = shm_to_rgba(&buf, 2, 1, 8);
        assert_eq!(out.len(), 8);
        assert_eq!(&out[4..8], &[0, 0, 0, 255]);
    }
}
