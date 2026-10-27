// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Row-major sub-tensor selection without copying: yields the contiguous byte chunks of a slice.

use crate::tensor::TensorView;
use std::fmt;
use std::ops::{
    Bound, Range, RangeBounds, RangeFrom, RangeFull, RangeInclusive, RangeTo, RangeToInclusive,
};

/// Why a slice request was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum InvalidSlice {
    /// More indexers than tensor dimensions.
    TooManySlices,
    /// Index/range past the dimension (or start after stop).
    SliceOutOfRange {
        dim_index: usize,
        asked: usize,
        dim_size: usize,
    },
    /// Sub-byte dtype chunk that does not start/end on a byte boundary.
    MisalignedSlice,
}

impl fmt::Display for InvalidSlice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InvalidSlice::TooManySlices => write!(f, "more slices than tensor dimensions"),
            InvalidSlice::SliceOutOfRange {
                dim_index,
                asked,
                dim_size,
            } => write!(f, "dim {dim_index}: asked {asked}, size {dim_size}"),
            InvalidSlice::MisalignedSlice => write!(f, "sub-byte slice not byte aligned"),
        }
    }
}

impl std::error::Error for InvalidSlice {}

/// One per-dimension selector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TensorIndexer {
    /// Pick one index; the dimension is dropped from the result shape.
    Select(usize),
    /// Keep a sub-range of the dimension.
    Narrow(Bound<usize>, Bound<usize>),
}

impl From<usize> for TensorIndexer {
    fn from(i: usize) -> Self {
        TensorIndexer::Select(i)
    }
}

macro_rules! from_range {
    ($t:ty) => {
        impl From<$t> for TensorIndexer {
            fn from(r: $t) -> Self {
                TensorIndexer::Narrow(r.start_bound().cloned(), r.end_bound().cloned())
            }
        }
    };
}
from_range!(Range<usize>);
from_range!(RangeFrom<usize>);
from_range!(RangeFull);
from_range!(RangeInclusive<usize>);
from_range!(RangeTo<usize>);
from_range!(RangeToInclusive<usize>);

/// `view.slice(idx)` for 1..=4 indexers (`a`, `(a,)`, `(a, b)`, ...).
pub trait IndexOp<'data, T> {
    fn slice(&'data self, index: T) -> Result<SliceIterator<'data>, InvalidSlice>;
}

impl<'data, A: Into<TensorIndexer>> IndexOp<'data, A> for TensorView<'data> {
    fn slice(&'data self, index: A) -> Result<SliceIterator<'data>, InvalidSlice> {
        self.sliced_data(&[index.into()])
    }
}

impl<'data, A: Into<TensorIndexer>> IndexOp<'data, (A,)> for TensorView<'data> {
    fn slice(&'data self, index: (A,)) -> Result<SliceIterator<'data>, InvalidSlice> {
        self.sliced_data(&[index.0.into()])
    }
}

impl<'data, A, B> IndexOp<'data, (A, B)> for TensorView<'data>
where
    A: Into<TensorIndexer>,
    B: Into<TensorIndexer>,
{
    fn slice(&'data self, index: (A, B)) -> Result<SliceIterator<'data>, InvalidSlice> {
        self.sliced_data(&[index.0.into(), index.1.into()])
    }
}

impl<'data, A, B, C> IndexOp<'data, (A, B, C)> for TensorView<'data>
where
    A: Into<TensorIndexer>,
    B: Into<TensorIndexer>,
    C: Into<TensorIndexer>,
{
    fn slice(&'data self, index: (A, B, C)) -> Result<SliceIterator<'data>, InvalidSlice> {
        self.sliced_data(&[index.0.into(), index.1.into(), index.2.into()])
    }
}

impl<'data, A, B, C, D> IndexOp<'data, (A, B, C, D)> for TensorView<'data>
where
    A: Into<TensorIndexer>,
    B: Into<TensorIndexer>,
    C: Into<TensorIndexer>,
    D: Into<TensorIndexer>,
{
    fn slice(&'data self, index: (A, B, C, D)) -> Result<SliceIterator<'data>, InvalidSlice> {
        self.sliced_data(&[
            index.0.into(),
            index.1.into(),
            index.2.into(),
            index.3.into(),
        ])
    }
}

/// Lazily yields the contiguous byte chunks (in row-major order) of a sliced tensor.
#[derive(Debug, Clone)]
pub struct SliceIterator<'data> {
    data: &'data [u8],
    /// Per outer dimension (before the chunk dimension): (start, stop, stride in elements).
    outer: Vec<(usize, usize, usize)>,
    /// Odometer over `outer`; `None` once exhausted.
    pos: Option<Vec<usize>>,
    /// Element offset of the chunk inside one outer position.
    base: usize,
    chunk_elems: usize,
    bits: usize,
    remaining: usize,
    newshape: Vec<usize>,
}

impl<'data> SliceIterator<'data> {
    pub(crate) fn new(
        view: &'data TensorView<'data>,
        slices: &[TensorIndexer],
    ) -> Result<Self, InvalidSlice> {
        let shape = view.shape();
        if slices.len() > shape.len() {
            return Err(InvalidSlice::TooManySlices);
        }
        let mut ranges = Vec::with_capacity(shape.len());
        let mut newshape = Vec::with_capacity(shape.len());
        for (i, &size) in shape.iter().enumerate() {
            let oor = |asked| InvalidSlice::SliceOutOfRange {
                dim_index: i,
                asked,
                dim_size: size,
            };
            match slices.get(i) {
                Some(TensorIndexer::Select(j)) => {
                    if *j >= size {
                        return Err(oor(*j));
                    }
                    ranges.push((*j, *j + 1));
                }
                other => {
                    let (lo, hi) = match other {
                        Some(TensorIndexer::Narrow(lo, hi)) => (*lo, *hi),
                        _ => (Bound::Unbounded, Bound::Unbounded),
                    };
                    let start = match lo {
                        Bound::Included(s) => s,
                        Bound::Excluded(s) => s.checked_add(1).ok_or(oor(s))?,
                        Bound::Unbounded => 0,
                    };
                    let stop = match hi {
                        Bound::Included(e) => e.checked_add(1).ok_or(oor(e))?,
                        Bound::Excluded(e) => e,
                        Bound::Unbounded => size,
                    };
                    if stop > size {
                        return Err(oor(stop));
                    }
                    if start > stop {
                        return Err(oor(start));
                    }
                    ranges.push((start, stop));
                    newshape.push(stop - start);
                }
            }
        }
        // Element strides (row-major). Shapes were validated at load, so the products fit usize.
        let mut strides = vec![1usize; shape.len()];
        for i in (0..shape.len().saturating_sub(1)).rev() {
            strides[i] = strides[i + 1] * shape[i + 1];
        }
        // Chunk dimension: innermost dim that is not taken whole; everything inside it is contiguous.
        let k = (0..shape.len()).rev().find(|&i| ranges[i] != (0, shape[i]));
        let bits = view.dtype().bitsize();
        let total_elems: usize = ranges.iter().map(|(a, b)| b - a).product();
        let (outer, base, chunk_elems) = match k {
            None => (Vec::new(), 0, total_elems),
            Some(k) => {
                let outer = (0..k)
                    .map(|i| (ranges[i].0, ranges[i].1, strides[i]))
                    .collect::<Vec<_>>();
                let base = ranges[k].0 * strides[k];
                let chunk = (ranges[k].1 - ranges[k].0) * strides[k];
                (outer, base, chunk)
            }
        };
        let empty = total_elems == 0;
        if !empty {
            // All chunk starts differ by multiples of strides; check every term is byte aligned.
            let aligned = |e: usize| (e * bits).is_multiple_of(8);
            if !aligned(chunk_elems)
                || !aligned(base)
                || outer
                    .iter()
                    .any(|&(s, _, st)| !aligned(st) || !aligned(s * st))
            {
                return Err(InvalidSlice::MisalignedSlice);
            }
        }
        let pos = if empty {
            None
        } else {
            Some(outer.iter().map(|o| o.0).collect())
        };
        Ok(SliceIterator {
            data: view.data(),
            outer,
            pos,
            base,
            chunk_elems,
            bits,
            remaining: total_elems * bits / 8,
            newshape,
        })
    }

    /// Bytes still to be yielded.
    pub fn remaining_byte_len(&self) -> usize {
        self.remaining
    }

    /// Shape of the resulting tensor (selected dims removed).
    pub fn newshape(&self) -> Vec<usize> {
        self.newshape.clone()
    }
}

impl<'data> Iterator for SliceIterator<'data> {
    type Item = &'data [u8];

    fn next(&mut self) -> Option<Self::Item> {
        let pos = self.pos.as_mut()?;
        let start_elem = self.base
            + pos
                .iter()
                .zip(self.outer.iter())
                .map(|(p, o)| p * o.2)
                .sum::<usize>();
        let b = start_elem * self.bits / 8;
        let e = b + self.chunk_elems * self.bits / 8;
        // Advance the odometer (last outer dim fastest).
        let mut done = true;
        for i in (0..pos.len()).rev() {
            pos[i] += 1;
            if pos[i] < self.outer[i].1 {
                done = false;
                break;
            }
            pos[i] = self.outer[i].0;
        }
        if done {
            self.pos = None;
        }
        self.remaining -= e - b;
        Some(&self.data[b..e])
    }
}
