// File: src/loader.rs
// Purpose: Photonio — non-blocking background decoder with session generations
//          and two dedicated worker threads: one for full-res images (priority)
//          and one for thumbnails (best-effort), so cheap thumbnails are never
//          starved behind slow full decodes.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, sync_channel, Receiver, Sender, SyncSender};
use std::sync::Arc;

use eframe::egui;

use crate::viewer;

/// Display-side cap for cached textures (matches egui's default GPU limit).
const DISPLAY_MAX_SIDE: u32 = 2048;
/// Square thumbnail target size.
const THUMB_SIZE: u32 = 96;
/// Hard cap on queued thumbnail requests; older requests are dropped.
const THUMB_QUEUE_CAP: usize = 8;
/// Bounded prefix read for EXIF-thumbnail extraction (avoids reading a huge
/// file just to make a 96px tile).
const THUMB_PREFIX_LIMIT: usize = 128 * 1024;

pub struct Request {
    pub generation: u64,
    pub kind: RequestKind,
}

pub enum RequestKind {
    Full { index: usize, path: PathBuf, len: usize },
    Thumb { index: usize, path: PathBuf },
}

pub struct Response {
    pub generation: u64,
    pub kind: ResponseKind,
}

/// Which animation format produced a multi-frame decode.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnimKind {
    Gif,
    WebP,
}

impl AnimKind {
    /// HUD badge label for this animation type.
    pub fn badge(&self) -> &'static str {
        match self {
            AnimKind::Gif => "[GIF]",
            AnimKind::WebP => "[WEBP ANIM]",
        }
    }
}

pub enum ResponseKind {
    Full {
        index: usize,
        rgba: Vec<u8>,
        display: [usize; 2],
        full: [usize; 2],
    },
    FullFailed { index: usize, error: String },
    /// A multi-frame animated image (GIF or WebP): display-res RGBA buffers
    /// with their display dimensions, the shared canvas dimensions, and
    /// per-frame delays.
    FullAnimated {
        index: usize,
        frames: Vec<(Vec<u8>, [usize; 2], [usize; 2])>,
        delays_ms: Vec<u64>,
        kind: AnimKind,
    },
    /// The full decode was coalesced away during fast navigation. The app
    /// must clear its pending flag so it can re-request if still needed.
    FullSkipped { index: usize },
    Thumb { index: usize, rgba: Vec<u8>, dims: [usize; 2] },
    ThumbFailed { index: usize },
}

pub struct Loader {
    full_tx: Sender<Request>,
    thumb_tx: SyncSender<Request>,
    rx: Receiver<Response>,
    generation: Arc<AtomicU64>,
    current_index: Arc<AtomicUsize>,
}

impl Loader {
    pub fn new(ctx: egui::Context) -> Self {
        let (resp_tx, resp_rx) = channel::<Response>();
        let (full_tx, full_rx) = channel::<Request>();
        let (thumb_tx, thumb_rx) = sync_channel::<Request>(THUMB_QUEUE_CAP);
        let generation = Arc::new(AtomicU64::new(0));
        let full_generation = Arc::clone(&generation);
        let thumb_generation = Arc::clone(&generation);
        let current_index = Arc::new(AtomicUsize::new(0));
        let full_current = Arc::clone(&current_index);
        let full_worker_ctx = ctx.clone();
        let thumb_worker_ctx = ctx;

        // Full-res worker (its own thread; decodes the active viewport).
        let full_resp_tx = resp_tx.clone();
        std::thread::spawn(move || {
            for req in full_rx {
                let Request { generation, kind } = req;
                if stale(&full_generation, generation) {
                    continue; // outdated → discard before decoding
                }
                let RequestKind::Full { index, path, len } = kind else {
                    continue;
                };
                // Coalesce during fast navigation: skip fulls that are no
                // longer the active (or immediately adjacent) index, so the
                // current image is decoded promptly and the queue stays lean.
                // Ring distance respects wrapping neighbors at folder edges.
                if ring_dist(index, full_current.load(Ordering::SeqCst), len) > 1 {
                    // Report the skip so the app clears `pending_full`; otherwise
                    // the index stays "in flight" forever and never re-decodes.
                    let _ = full_resp_tx.send(Response {
                        generation,
                        kind: ResponseKind::FullSkipped { index },
                    });
                    full_worker_ctx.request_repaint();
                    continue;
                }
                let kind = match decode_full(&path) {
                    Ok(DecodedFull::Static { rgba, display, full }) => {
                        ResponseKind::Full { index, rgba, display, full }
                    }
                    Ok(DecodedFull::Animated { frames, delays_ms, kind }) => {
                        ResponseKind::FullAnimated { index, frames, delays_ms, kind }
                    }
                    Err(error) => ResponseKind::FullFailed { index, error },
                };
                let _ = full_resp_tx.send(Response { generation, kind });
                full_worker_ctx.request_repaint();
            }
        });

        // Thumbnail worker (separate thread, never blocked by full decodes).
        std::thread::spawn(move || {
            for req in thumb_rx {
                let Request { generation, kind } = req;
                if stale(&thumb_generation, generation) {
                    continue;
                }
                let RequestKind::Thumb { index, path } = kind else {
                    continue;
                };
                let kind = match decode_thumb(&path) {
                    Ok((rgba, dims)) => ResponseKind::Thumb { index, rgba, dims },
                    Err(_) => ResponseKind::ThumbFailed { index },
                };
                let _ = resp_tx.send(Response { generation, kind });
                thumb_worker_ctx.request_repaint();
            }
        });

        Self { full_tx, thumb_tx, rx: resp_rx, generation, current_index }
    }

    /// Current session generation id.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// Tell the worker which index is currently being viewed (for coalescing).
    pub fn set_current_index(&self, index: usize) {
        self.current_index.store(index, Ordering::SeqCst);
    }

    /// Bump the generation, invalidating all in-flight/queued work.
    pub fn bump_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn request_full(&self, index: usize, path: PathBuf, len: usize) {
        let generation = self.generation.load(Ordering::SeqCst);
        let _ = self.full_tx.send(Request {
            generation,
            kind: RequestKind::Full { index, path, len },
        });
    }

    /// Best-effort thumbnail request. Returns `true` if it was queued, or
    /// `false` if the bounded queue was full (caller should retry later).
    pub fn request_thumb(&self, index: usize, path: PathBuf) -> bool {
        let generation = self.generation.load(Ordering::SeqCst);
        self.thumb_tx
            .try_send(Request {
                generation,
                kind: RequestKind::Thumb { index, path },
            })
            .is_ok()
    }

    /// Non-blocking poll for the next ready response.
    pub fn try_recv(&self) -> Option<Response> {
        self.rx.try_recv().ok()
    }
}

/// True if `req_gen` is older than the current generation (stale).
fn stale(current: &AtomicU64, req_gen: u64) -> bool {
    req_gen < current.load(Ordering::SeqCst)
}

/// Circular (ring) distance between two indices in a list of length `len`.
/// Treats wrapping neighbors (e.g. `0` and `len - 1`) as adjacent, so caching
/// and coalescing don't thrash or leave gaps at the folder boundaries.
#[inline]
pub(crate) fn ring_dist(a: usize, b: usize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let d = a.abs_diff(b);
    d.min(len.saturating_sub(d))
}

/// A finished full-res decode: either a single static image or an animation.
enum DecodedFull {
    Static { rgba: Vec<u8>, display: [usize; 2], full: [usize; 2] },
    Animated {
        frames: Vec<(Vec<u8>, [usize; 2], [usize; 2])>,
        delays_ms: Vec<u64>,
        kind: AnimKind,
    },
}

fn is_gif(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("gif"))
        .unwrap_or(false)
}

fn is_webp(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("webp"))
        .unwrap_or(false)
}

fn decode_full(path: &Path) -> Result<DecodedFull, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;

    // Animated GIF or WebP: decode all frames once. Single-frame images are
    // handled as static; an animation that fails to decode falls back to the
    // plain static decoder below.
    let kind = if is_gif(path) {
        Some(AnimKind::Gif)
    } else if is_webp(path) {
        Some(AnimKind::WebP)
    } else {
        None
    };

    if let Some(kind) = kind {
        let decoded = match kind {
            AnimKind::Gif => viewer::decode_gif(&bytes),
            AnimKind::WebP => viewer::decode_webp(&bytes),
        };
        if let Ok(decoded) = decoded {
            let viewer::DecodedAnimation { frames, delays } = decoded;
            if frames.len() > 1 {
                let mut out_frames = Vec::with_capacity(frames.len());
                let mut out_delays = Vec::with_capacity(delays.len());
                for (frame, delay) in frames.into_iter().zip(delays.into_iter()) {
                    let full = [frame.width() as usize, frame.height() as usize];
                    let fitted = viewer::fit_within(&frame, DISPLAY_MAX_SIDE);
                    let display = [fitted.width() as usize, fitted.height() as usize];
                    let rgba = fitted.to_rgba8().into_raw();
                    out_frames.push((rgba, display, full));
                    // Clamp to a sane minimum so a 0ms delay can't busy-loop.
                    out_delays.push((delay.as_millis() as u64).max(10));
                }
                return Ok(DecodedFull::Animated {
                    frames: out_frames,
                    delays_ms: out_delays,
                    kind,
                });
            }
            if let Some(single) = frames.into_iter().next() {
                let full = [single.width() as usize, single.height() as usize];
                let fitted = viewer::fit_within(&single, DISPLAY_MAX_SIDE);
                let display = [fitted.width() as usize, fitted.height() as usize];
                let rgba = fitted.to_rgba8().into_raw();
                return Ok(DecodedFull::Static { rgba, display, full });
            }
        }
    }

    let img = viewer::decode(&bytes)?; // EXIF-oriented DynamicImage
    let full = [img.width() as usize, img.height() as usize];
    let img = viewer::fit_within(&img, DISPLAY_MAX_SIDE);
    let display = [img.width() as usize, img.height() as usize];
    let rgba = img.to_rgba8().into_raw();
    Ok(DecodedFull::Static { rgba, display, full })
}

fn decode_thumb(path: &Path) -> Result<(Vec<u8>, [usize; 2]), String> {
    // Fast path: read only a bounded prefix and use the embedded EXIF thumbnail
    // (a few KB) instead of decompressing the whole multi-megapixel image.
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut prefix = vec![0u8; THUMB_PREFIX_LIMIT];
    let n = file.read(&mut prefix).map_err(|e| e.to_string())?;
    prefix.truncate(n);

    if let Some(thumb) = viewer::embedded_thumbnail(&prefix, THUMB_SIZE) {
        let dims = [thumb.width() as usize, thumb.height() as usize];
        return Ok((thumb.to_rgba8().into_raw(), dims));
    }

    // Fallback (rare): no EXIF thumbnail — full read + bounded full decode.
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let thumb = viewer::decode_thumbnail(&bytes, THUMB_SIZE)?;
    let dims = [thumb.width() as usize, thumb.height() as usize];
    Ok((thumb.to_rgba8().into_raw(), dims))
}
