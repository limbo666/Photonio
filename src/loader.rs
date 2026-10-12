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
    Full { index: usize, path: PathBuf },
    Thumb { index: usize, path: PathBuf },
}

pub struct Response {
    pub generation: u64,
    pub kind: ResponseKind,
}

pub enum ResponseKind {
    Full {
        index: usize,
        rgba: Vec<u8>,
        display: [usize; 2],
        full: [usize; 2],
    },
    FullFailed { index: usize, error: String },
    /// The full decode was coalesced away during fast navigation. The app
    /// must clear its pending flag so it can re-request if still needed.
    FullSkipped { index: usize },
    Thumb { index: usize, rgba: Vec<u8>, dims: [usize; 2] },
    ThumbFailed,
}

pub struct Loader {
    full_tx: Sender<Request>,
    thumb_tx: SyncSender<Request>,
    rx: Receiver<Response>,
    generation: Arc<AtomicU64>,
    current_index: Arc<AtomicUsize>,
}

impl Loader {
    pub fn new() -> Self {
        let (resp_tx, resp_rx) = channel::<Response>();
        let (full_tx, full_rx) = channel::<Request>();
        let (thumb_tx, thumb_rx) = sync_channel::<Request>(THUMB_QUEUE_CAP);
        let generation = Arc::new(AtomicU64::new(0));
        let full_generation = Arc::clone(&generation);
        let thumb_generation = Arc::clone(&generation);
        let current_index = Arc::new(AtomicUsize::new(0));
        let full_current = Arc::clone(&current_index);

        // Full-res worker (its own thread; decodes the active viewport).
        let full_resp_tx = resp_tx.clone();
        std::thread::spawn(move || {
            for req in full_rx {
                let Request { generation, kind } = req;
                if stale(&full_generation, generation) {
                    continue; // outdated → discard before decoding
                }
                let RequestKind::Full { index, path } = kind else {
                    continue;
                };
                // Coalesce during fast navigation: skip fulls that are no
                // longer the active (or immediately adjacent) index, so the
                // current image is decoded promptly and the queue stays lean.
                if (index as isize - full_current.load(Ordering::SeqCst) as isize).abs() > 1 {
                    // Report the skip so the app clears `pending_full`; otherwise
                    // the index stays "in flight" forever and never re-decodes.
                    let _ = full_resp_tx.send(Response {
                        generation,
                        kind: ResponseKind::FullSkipped { index },
                    });
                    continue;
                }
                let kind = match decode_full(&path) {
                    Ok((rgba, display, full)) => {
                        ResponseKind::Full { index, rgba, display, full }
                    }
                    Err(error) => ResponseKind::FullFailed { index, error },
                };
                let _ = full_resp_tx.send(Response { generation, kind });
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
                    Err(_) => ResponseKind::ThumbFailed,
                };
                let _ = resp_tx.send(Response { generation, kind });
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

    pub fn request_full(&self, index: usize, path: PathBuf) {
        let generation = self.generation.load(Ordering::SeqCst);
        let _ = self.full_tx.send(Request {
            generation,
            kind: RequestKind::Full { index, path },
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

fn decode_full(path: &Path) -> Result<(Vec<u8>, [usize; 2], [usize; 2]), String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let img = viewer::decode(&bytes)?; // EXIF-oriented DynamicImage
    let full = [img.width() as usize, img.height() as usize];
    let img = viewer::fit_within(&img, DISPLAY_MAX_SIDE);
    let display = [img.width() as usize, img.height() as usize];
    let rgba = img.to_rgba8().into_raw();
    Ok((rgba, display, full))
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
