// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Reordering jitter buffer for the playout (model -> user) stream.
//!
//! Frames arrive with sequence numbers, possibly late, duplicated or out of
//! order. Playout starts once `target_depth` frames are buffered (prebuffer),
//! then pops exactly one frame per tick. A missing frame is concealed with
//! silence (simple PLC); a frame arriving after its slot played is dropped.
//! `flush()` is what barge-in calls: the model's queued speech is discarded at
//! once so the user is not talked over.

use std::collections::BTreeMap;

use crate::audio::Frame;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JitterStats {
    pub pushed: u64,
    pub played: u64,
    pub concealed: u64,
    pub late_dropped: u64,
    pub duplicates: u64,
    pub overflow_dropped: u64,
    pub flushed: u64,
}

#[derive(Debug)]
pub struct JitterBuffer {
    frame_samples: usize,
    target_depth: usize,
    max_depth: usize,
    frames: BTreeMap<u64, Frame>,
    next_play: Option<u64>,
    primed: bool,
    stats: JitterStats,
}

impl JitterBuffer {
    /// `target_depth` frames of prebuffer; `max_depth` caps memory and latency.
    pub fn new(frame_samples: usize, target_depth: usize, max_depth: usize) -> Self {
        let target_depth = target_depth.max(1);
        JitterBuffer {
            frame_samples,
            target_depth,
            max_depth: max_depth.max(target_depth),
            frames: BTreeMap::new(),
            next_play: None,
            primed: false,
            stats: JitterStats::default(),
        }
    }

    pub fn stats(&self) -> JitterStats {
        self.stats
    }

    pub fn depth(&self) -> usize {
        self.frames.len()
    }

    pub fn is_playing(&self) -> bool {
        self.primed
    }

    pub fn push(&mut self, frame: Frame) {
        if let Some(np) = self.next_play {
            if frame.seq < np {
                self.stats.late_dropped += 1;
                return;
            }
        }
        if self.frames.contains_key(&frame.seq) {
            self.stats.duplicates += 1;
            return;
        }
        self.stats.pushed += 1;
        self.frames.insert(frame.seq, frame);
        while self.frames.len() > self.max_depth {
            // Too far behind: drop the oldest to bound latency.
            if let Some((&k, _)) = self.frames.iter().next() {
                self.frames.remove(&k);
                self.stats.overflow_dropped += 1;
                self.next_play = Some(k + 1);
            }
        }
    }

    /// One playout tick. `None` while prebuffering or idle.
    pub fn pop(&mut self) -> Option<Frame> {
        if !self.primed {
            if self.frames.len() < self.target_depth {
                return None;
            }
            self.primed = true;
            // (Re)start at the oldest buffered frame: a gap before a new talk
            // spurt (after underrun or flush) is not loss, so not concealed.
            let first = self.frames.keys().next().copied();
            self.next_play = match (self.next_play, first) {
                (Some(np), Some(f)) => Some(np.max(f)),
                (np, f) => f.or(np),
            };
        }
        if self.frames.is_empty() {
            // Underrun: stream paused or ended; re-prebuffer next time.
            self.primed = false;
            return None;
        }
        let seq = self.next_play.unwrap_or(0);
        self.next_play = Some(seq + 1);
        match self.frames.remove(&seq) {
            Some(f) => {
                self.stats.played += 1;
                Some(f)
            }
            None => {
                self.stats.concealed += 1;
                Some(Frame::silence(seq, self.frame_samples))
            }
        }
    }

    /// Discard everything queued (barge-in). Frames older than the newest
    /// discarded one will be treated as late if they straggle in.
    pub fn flush(&mut self) {
        let n = self.frames.len() as u64;
        if let Some((&last, _)) = self.frames.iter().next_back() {
            self.next_play = Some(last + 1);
        }
        self.frames.clear();
        self.primed = false;
        self.stats.flushed += n;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(seq: u64) -> Frame {
        Frame {
            seq,
            samples: vec![seq as i16; 4],
        }
    }

    #[test]
    fn reorders_conceals_and_drops_late() {
        let mut j = JitterBuffer::new(4, 3, 16);
        j.push(f(1));
        j.push(f(0));
        assert_eq!(j.pop(), None, "prebuffering");
        j.push(f(3)); // 2 is missing
        j.push(f(3)); // duplicate
        assert_eq!(j.pop().unwrap().seq, 0);
        assert_eq!(j.pop().unwrap().seq, 1);
        let c = j.pop().unwrap();
        assert_eq!((c.seq, c.samples.clone()), (2, vec![0; 4]), "concealed");
        j.push(f(2)); // straggler after its slot
        assert_eq!(j.pop().unwrap().seq, 3);
        assert_eq!(j.pop(), None, "underrun");
        let s = j.stats();
        assert_eq!(
            (s.played, s.concealed, s.late_dropped, s.duplicates),
            (3, 1, 1, 1)
        );
    }

    #[test]
    fn flush_discards_and_overflow_bounds_latency() {
        let mut j = JitterBuffer::new(4, 1, 3);
        for s in 0..5 {
            j.push(f(s));
        }
        assert_eq!(j.depth(), 3);
        assert_eq!(j.stats().overflow_dropped, 2);
        assert_eq!(j.pop().unwrap().seq, 2);
        j.flush();
        assert_eq!(j.depth(), 0);
        assert_eq!(j.stats().flushed, 2);
        j.push(f(3)); // stale after flush
        assert_eq!(j.stats().late_dropped, 1);
        j.push(f(5));
        assert_eq!(j.pop().unwrap().seq, 5);
        assert!(j.is_playing());
    }
}
