use siphon_core::{SegmentInfo, SegmentState};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const STATE_PENDING: u8 = 0;
const STATE_ACTIVE: u8 = 1;
const STATE_RETRYING: u8 = 2;
const STATE_DONE: u8 = 3;
const STATE_ERROR: u8 = 4;

fn state_to_u8(s: SegmentState) -> u8 {
    match s {
        SegmentState::Pending => STATE_PENDING,
        SegmentState::Active => STATE_ACTIVE,
        SegmentState::Retrying => STATE_RETRYING,
        SegmentState::Done => STATE_DONE,
        SegmentState::Error => STATE_ERROR,
    }
}

fn u8_to_state(v: u8) -> SegmentState {
    match v {
        STATE_PENDING => SegmentState::Pending,
        STATE_ACTIVE => SegmentState::Active,
        STATE_RETRYING => SegmentState::Retrying,
        STATE_DONE => SegmentState::Done,
        _ => SegmentState::Error,
    }
}

#[derive(Debug, Clone, Copy)]
struct SegmentBounds {
    current: u64,
    end: u64,
}

/// Thread-safe live segment with mathematically airtight bounds for dynamic splitting.
pub struct LiveSegment {
    pub index: u32,
    pub start_offset: u64,
    bounds: Mutex<SegmentBounds>,
    state: AtomicU8,
    claimed: AtomicBool,
    window_bytes: AtomicU64,
    speed_bps: AtomicU64,
}

impl LiveSegment {
    pub fn new(index: u32, start: u64, current: u64, end: u64, state: SegmentState) -> Arc<Self> {
        Arc::new(Self {
            index,
            start_offset: start,
            bounds: Mutex::new(SegmentBounds { current, end }),
            state: AtomicU8::new(state_to_u8(state)),
            claimed: AtomicBool::new(state == SegmentState::Done),
            window_bytes: AtomicU64::new(0),
            speed_bps: AtomicU64::new(0),
        })
    }

    pub fn set_state(&self, s: SegmentState) {
        self.state.store(state_to_u8(s), Ordering::SeqCst);
        if s != SegmentState::Active {
            self.speed_bps.store(0, Ordering::Relaxed);
        }
    }

    pub fn state(&self) -> SegmentState {
        u8_to_state(self.state.load(Ordering::SeqCst))
    }

    pub fn current_and_end(&self) -> (u64, u64) {
        let b = self.bounds.lock().expect("segment bounds lock poisoned");
        (b.current, b.end)
    }

    /// Reserve up to `chunk_len` bytes starting at `current`, clamped against dynamic `end`.
    /// Returns `Some((write_offset, allowed_len, reached_end))` or `None` if already completed.
    pub fn reserve_write(&self, chunk_len: usize) -> Option<(u64, usize, bool)> {
        let mut b = self.bounds.lock().expect("segment bounds lock poisoned");
        if b.end != u64::MAX && b.current > b.end {
            return None;
        }
        let allowed = if b.end == u64::MAX {
            chunk_len
        } else {
            let rem = b.end - b.current + 1;
            rem.min(chunk_len as u64) as usize
        };
        if allowed == 0 {
            return None;
        }
        let offset = b.current;
        b.current = b.current.saturating_add(allowed as u64);
        let reached_end = b.end != u64::MAX && b.current > b.end;
        Some((offset, allowed, reached_end))
    }

    pub fn record_written(&self, bytes: u64) {
        self.window_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Atomically try to split the remaining work of this segment in half.
    /// Returns `Some((new_start, new_end))` if remaining >= `min_split_size`.
    pub fn try_split_tail(&self, min_split_size: u64) -> Option<(u64, u64)> {
        if self.state() != SegmentState::Active {
            return None;
        }
        let mut b = self.bounds.lock().expect("segment bounds lock poisoned");
        if b.end == u64::MAX || b.current > b.end {
            return None;
        }
        let remaining = b.end - b.current + 1;
        if remaining < min_split_size.max(2) {
            return None;
        }
        let split_point = b.current + remaining / 2;
        if split_point <= b.current || split_point > b.end {
            return None;
        }
        let stolen_end = b.end;
        b.end = split_point - 1;
        Some((split_point, stolen_end))
    }

    pub fn remaining_bytes(&self) -> u64 {
        let b = self.bounds.lock().expect("segment bounds lock poisoned");
        if b.end == u64::MAX {
            u64::MAX
        } else if b.current > b.end {
            0
        } else {
            b.end - b.current + 1
        }
    }

    pub fn snapshot(&self, elapsed_secs: f64) -> SegmentInfo {
        let (cur, end) = self.current_and_end();
        let bytes = self.window_bytes.swap(0, Ordering::Relaxed);
        let speed = if elapsed_secs > 0.001 && self.state() == SegmentState::Active {
            (bytes as f64 / elapsed_secs) as u64
        } else {
            0
        };
        self.speed_bps.store(speed, Ordering::Relaxed);

        SegmentInfo {
            index: self.index,
            start_offset: self.start_offset,
            current_offset: cur,
            end_offset: end,
            state: self.state(),
            speed_bps: speed,
        }
    }
}

/// Coordinates all segments of a single HTTP download, including dynamic work stealing.
pub struct SegmentCoordinator {
    segments: Mutex<Vec<Arc<LiveSegment>>>,
    supports_range: bool,
    min_split_size: u64,
    total_size: Option<u64>,
}

impl SegmentCoordinator {
    pub fn initialize(
        existing: &[SegmentInfo],
        total_size: Option<u64>,
        supports_range: bool,
        initial_connections: u32,
        min_split_size: u64,
    ) -> Arc<Self> {
        let min_split = min_split_size.max(32 * 1024);

        let segments = if !existing.is_empty() && supports_range {
            existing
                .iter()
                .map(|s| {
                    let is_done = s.state == SegmentState::Done
                        || (s.end_offset != u64::MAX && s.current_offset > s.end_offset);
                    let state = if is_done {
                        SegmentState::Done
                    } else {
                        SegmentState::Pending
                    };
                    LiveSegment::new(
                        s.index,
                        s.start_offset,
                        s.current_offset,
                        s.end_offset,
                        state,
                    )
                })
                .collect()
        } else {
            match (supports_range, total_size) {
                (true, Some(total)) if total > 0 => {
                    let max_useful = (total / min_split).max(1) as u32;
                    let count = initial_connections.clamp(1, 32).min(max_useful).max(1);
                    let chunk = total / count as u64;
                    let mut list = Vec::with_capacity(count as usize);
                    for i in 0..count {
                        let start = i as u64 * chunk;
                        let end = if i + 1 == count {
                            total - 1
                        } else {
                            (i as u64 + 1) * chunk - 1
                        };
                        list.push(LiveSegment::new(i, start, start, end, SegmentState::Pending));
                    }
                    list
                }
                (_, Some(0)) => vec![],
                (_, Some(total)) => {
                    vec![LiveSegment::new(
                        0,
                        0,
                        0,
                        total - 1,
                        SegmentState::Pending,
                    )]
                }
                (_, None) => {
                    vec![LiveSegment::new(
                        0,
                        0,
                        0,
                        u64::MAX,
                        SegmentState::Pending,
                    )]
                }
            }
        };

        Arc::new(Self {
            segments: Mutex::new(segments),
            supports_range,
            min_split_size: min_split,
            total_size,
        })
    }

    pub fn supports_range(&self) -> bool {
        self.supports_range
    }

    pub fn total_size(&self) -> Option<u64> {
        self.total_size
    }

    /// Claim the next pending segment, or dynamically steal the back half of the
    /// largest active segment (`remaining >= min_split_size`).
    pub fn claim_or_steal(&self) -> Option<(Arc<LiveSegment>, bool)> {
        let mut list = self.segments.lock().expect("segments lock poisoned");

        // 1) First claim any unclaimed pending segment
        for seg in list.iter() {
            if seg.state() == SegmentState::Pending
                && !seg.claimed.swap(true, Ordering::SeqCst)
            {
                seg.set_state(SegmentState::Active);
                return Some((Arc::clone(seg), false));
            }
        }

        // 2) No pending segments: attempt dynamic work stealing if server supports Range
        if !self.supports_range {
            return None;
        }

        let victim = list
            .iter()
            .filter(|s| s.state() == SegmentState::Active)
            .max_by_key(|s| s.remaining_bytes())
            .cloned()?;

        let (stolen_start, stolen_end) = victim.try_split_tail(self.min_split_size)?;
        let next_idx = list.len() as u32;
        let new_seg = LiveSegment::new(
            next_idx,
            stolen_start,
            stolen_start,
            stolen_end,
            SegmentState::Active,
        );
        new_seg.claimed.store(true, Ordering::SeqCst);
        list.push(Arc::clone(&new_seg));
        Some((new_seg, true))
    }

    pub fn all_done(&self) -> bool {
        let list = self.segments.lock().expect("segments lock poisoned");
        list.iter().all(|s| s.state() == SegmentState::Done)
    }

    pub fn snapshot(&self, elapsed_secs: f64) -> (u64, u64, Vec<SegmentInfo>) {
        let list = self.segments.lock().expect("segments lock poisoned");
        let mut total_downloaded = 0u64;
        let mut total_speed = 0u64;
        let mut infos = Vec::with_capacity(list.len());
        for seg in list.iter() {
            let info = seg.snapshot(elapsed_secs);
            total_downloaded = total_downloaded.saturating_add(info.downloaded_bytes());
            total_speed = total_speed.saturating_add(info.speed_bps);
            infos.push(info);
        }
        (total_downloaded, total_speed, infos)
    }
}
