// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Header parsing/validation, zero-copy tensor views and serialization.

use crate::slice::{InvalidSlice, SliceIterator, TensorIndexer};
use serde::de::{Deserializer, IgnoredAny, MapAccess, Visitor};
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::io::Write;
use std::path::Path;

/// Largest JSON header accepted (bytes). Bounds memory used by a hostile file before any validation.
pub const MAX_HEADER_SIZE: usize = 100_000_000;
const METADATA_KEY: &str = "__metadata__";

/// Every failure this crate can report.
#[derive(Debug)]
#[non_exhaustive]
pub enum SafeTensorError {
    /// Buffer shorter than the 8-byte length prefix.
    HeaderTooSmall,
    /// Declared header length above [`MAX_HEADER_SIZE`].
    HeaderTooLarge,
    /// Declared header length runs past the end of the buffer.
    InvalidHeaderLength,
    /// Header bytes are not UTF-8.
    InvalidHeader(String),
    /// Header does not start with `{`.
    InvalidHeaderStart,
    /// Header is not the expected JSON shape.
    InvalidHeaderDeserialization(String),
    /// Same tensor name appears twice (header or serialize input).
    DuplicateName(String),
    /// Lookup of a name that is not in the file.
    TensorNotFound(String),
    /// Tensor byte range does not match `dtype` x `shape`.
    TensorInvalidInfo,
    /// Offsets are not contiguous from 0 (names the first bad tensor).
    InvalidOffset(String),
    /// Tensors do not cover the whole data buffer.
    MetadataIncompleteBuffer,
    /// An element/byte count overflowed `usize`.
    ValidationOverflow,
    /// Sub-byte dtype whose element count does not end on a byte boundary.
    MisalignedSlice,
    /// `TensorView::new` / `View` data length does not match dtype x shape.
    InvalidTensorView(Dtype, Vec<usize>, usize),
    /// File I/O error during `serialize_to_file`.
    IoError(std::io::Error),
}

impl fmt::Display for SafeTensorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use SafeTensorError::*;
        match self {
            HeaderTooSmall => write!(f, "safetensors: buffer shorter than 8-byte header length"),
            HeaderTooLarge => write!(f, "safetensors: header larger than {MAX_HEADER_SIZE} bytes"),
            InvalidHeaderLength => write!(f, "safetensors: header length past end of buffer"),
            InvalidHeader(e) => write!(f, "safetensors: header is not UTF-8: {e}"),
            InvalidHeaderStart => write!(f, "safetensors: header does not start with '{{'"),
            InvalidHeaderDeserialization(e) => write!(f, "safetensors: bad header JSON: {e}"),
            DuplicateName(n) => write!(f, "safetensors: duplicate tensor name {n:?}"),
            TensorNotFound(n) => write!(f, "safetensors: tensor {n:?} not found"),
            TensorInvalidInfo => write!(f, "safetensors: byte range does not match dtype x shape"),
            InvalidOffset(n) => write!(f, "safetensors: non-contiguous offsets at tensor {n:?}"),
            MetadataIncompleteBuffer => {
                write!(f, "safetensors: tensors do not cover the data buffer")
            }
            ValidationOverflow => write!(f, "safetensors: size overflow while validating"),
            MisalignedSlice => write!(f, "safetensors: sub-byte tensor not byte aligned"),
            InvalidTensorView(d, s, n) => {
                write!(f, "safetensors: {n} bytes do not fit dtype {d} shape {s:?}")
            }
            IoError(e) => write!(f, "safetensors: io: {e}"),
        }
    }
}

impl std::error::Error for SafeTensorError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SafeTensorError::IoError(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for SafeTensorError {
    fn from(e: std::io::Error) -> Self {
        SafeTensorError::IoError(e)
    }
}

/// Element types. Declared in increasing alignment order; `Ord` follows declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
#[allow(non_camel_case_types)]
pub enum Dtype {
    BOOL,
    F4,
    F6_E2M3,
    F6_E3M2,
    U8,
    I8,
    F8_E5M2,
    F8_E4M3,
    F8_E8M0,
    I16,
    U16,
    F16,
    BF16,
    I32,
    U32,
    F32,
    C64,
    F64,
    I64,
    U64,
}

const ALL_DTYPES: [Dtype; 20] = [
    Dtype::BOOL,
    Dtype::F4,
    Dtype::F6_E2M3,
    Dtype::F6_E3M2,
    Dtype::U8,
    Dtype::I8,
    Dtype::F8_E5M2,
    Dtype::F8_E4M3,
    Dtype::F8_E8M0,
    Dtype::I16,
    Dtype::U16,
    Dtype::F16,
    Dtype::BF16,
    Dtype::I32,
    Dtype::U32,
    Dtype::F32,
    Dtype::C64,
    Dtype::F64,
    Dtype::I64,
    Dtype::U64,
];

impl Dtype {
    /// Bits per element.
    pub fn bitsize(&self) -> usize {
        use Dtype::*;
        match self {
            F4 => 4,
            F6_E2M3 | F6_E3M2 => 6,
            BOOL | U8 | I8 | F8_E5M2 | F8_E4M3 | F8_E8M0 => 8,
            I16 | U16 | F16 | BF16 => 16,
            I32 | U32 | F32 => 32,
            C64 | F64 | I64 | U64 => 64,
        }
    }

    /// Bytes per element (0 for sub-byte types; use [`Dtype::bitsize`] there).
    pub fn size(&self) -> usize {
        self.bitsize() / 8
    }

    /// Wire name used in the JSON header.
    pub fn as_str(&self) -> &'static str {
        use Dtype::*;
        match self {
            BOOL => "BOOL",
            F4 => "F4",
            F6_E2M3 => "F6_E2M3",
            F6_E3M2 => "F6_E3M2",
            U8 => "U8",
            I8 => "I8",
            F8_E5M2 => "F8_E5M2",
            F8_E4M3 => "F8_E4M3",
            F8_E8M0 => "F8_E8M0",
            I16 => "I16",
            U16 => "U16",
            F16 => "F16",
            BF16 => "BF16",
            I32 => "I32",
            U32 => "U32",
            F32 => "F32",
            C64 => "C64",
            F64 => "F64",
            I64 => "I64",
            U64 => "U64",
        }
    }

    /// Parse a wire name (exact, case-sensitive).
    pub fn from_wire(s: &str) -> Option<Dtype> {
        ALL_DTYPES.iter().copied().find(|d| d.as_str() == s)
    }
}

impl fmt::Display for Dtype {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Number of bytes `shape` elements of `dtype` take, or an error if it overflows / is not whole bytes.
pub(crate) fn byte_len(dtype: Dtype, shape: &[usize]) -> Result<usize, SafeTensorError> {
    let elems = shape
        .iter()
        .try_fold(1usize, |acc, &d| acc.checked_mul(d))
        .ok_or(SafeTensorError::ValidationOverflow)?;
    let bits = elems
        .checked_mul(dtype.bitsize())
        .ok_or(SafeTensorError::ValidationOverflow)?;
    if !bits.is_multiple_of(8) {
        return Err(SafeTensorError::MisalignedSlice);
    }
    Ok(bits / 8)
}

/// Something that can be written as a tensor.
pub trait View {
    fn dtype(&self) -> Dtype;
    fn shape(&self) -> &[usize];
    fn data(&self) -> Cow<'_, [u8]>;
    fn data_len(&self) -> usize;
}

/// Per-tensor header entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TensorInfo {
    pub dtype: Dtype,
    pub shape: Vec<usize>,
    /// `[begin, end)` relative to the data buffer.
    pub data_offsets: (usize, usize),
}

/// Parsed and validated header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    metadata: Option<HashMap<String, String>>,
    /// Tensors in offset order.
    tensors: Vec<TensorInfo>,
    names: Vec<String>,
    index_map: HashMap<String, usize>,
}

impl Metadata {
    /// Tensor entry by name.
    pub fn info(&self, name: &str) -> Option<&TensorInfo> {
        self.index_map.get(name).map(|&i| &self.tensors[i])
    }

    /// All tensor entries keyed by name.
    pub fn tensors(&self) -> HashMap<String, &TensorInfo> {
        self.names
            .iter()
            .cloned()
            .zip(self.tensors.iter())
            .collect()
    }

    /// Names in offset order.
    pub fn offset_keys(&self) -> Vec<String> {
        self.names.clone()
    }

    /// Size of the data buffer the header describes.
    pub fn data_len(&self) -> usize {
        self.tensors.last().map(|t| t.data_offsets.1).unwrap_or(0)
    }

    /// Free-form `__metadata__` map, if present.
    pub fn metadata(&self) -> &Option<HashMap<String, String>> {
        &self.metadata
    }
}

/// Raw header as read: (name, info) in file order plus optional metadata. Duplicate keys are rejected
/// here, before any map can silently keep only one of them.
struct RawHeader {
    metadata: Option<HashMap<String, String>>,
    entries: Vec<(String, TensorInfo)>,
}

fn bad(msg: impl Into<String>) -> SafeTensorError {
    SafeTensorError::InvalidHeaderDeserialization(msg.into())
}

fn info_from_value(name: &str, v: serde_json::Value) -> Result<TensorInfo, SafeTensorError> {
    let obj = match v {
        serde_json::Value::Object(o) => o,
        _ => return Err(bad(format!("tensor {name:?}: entry is not an object"))),
    };
    let dtype = obj
        .get("dtype")
        .and_then(|d| d.as_str())
        .ok_or_else(|| bad(format!("tensor {name:?}: missing dtype")))?;
    let dtype = Dtype::from_wire(dtype)
        .ok_or_else(|| bad(format!("tensor {name:?}: unknown dtype {dtype:?}")))?;
    let as_usize = |x: &serde_json::Value| -> Option<usize> {
        x.as_u64().and_then(|u| usize::try_from(u).ok())
    };
    let shape = obj
        .get("shape")
        .and_then(|s| s.as_array())
        .ok_or_else(|| bad(format!("tensor {name:?}: missing shape")))?
        .iter()
        .map(as_usize)
        .collect::<Option<Vec<usize>>>()
        .ok_or_else(|| {
            bad(format!(
                "tensor {name:?}: shape must be non-negative integers"
            ))
        })?;
    let offs = obj
        .get("data_offsets")
        .and_then(|s| s.as_array())
        .ok_or_else(|| bad(format!("tensor {name:?}: missing data_offsets")))?;
    if offs.len() != 2 {
        return Err(bad(format!(
            "tensor {name:?}: data_offsets must have 2 items"
        )));
    }
    let (b, e) = match (as_usize(&offs[0]), as_usize(&offs[1])) {
        (Some(b), Some(e)) => (b, e),
        _ => return Err(bad(format!("tensor {name:?}: bad data_offsets"))),
    };
    Ok(TensorInfo {
        dtype,
        shape,
        data_offsets: (b, e),
    })
}

struct HeaderVisitor;

impl<'de> Visitor<'de> for HeaderVisitor {
    type Value = Result<RawHeader, SafeTensorError>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a safetensors header object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut seen = HashSet::new();
        let mut out = RawHeader {
            metadata: None,
            entries: Vec::new(),
        };
        // On the first semantic error keep draining (IgnoredAny) so the JSON parser still sees a
        // well-formed map end; then report that first error.
        let mut first_err: Option<SafeTensorError> = None;
        while let Some(key) = map.next_key::<String>()? {
            if first_err.is_some() {
                map.next_value::<IgnoredAny>()?;
                continue;
            }
            if !seen.insert(key.clone()) {
                map.next_value::<IgnoredAny>()?;
                first_err = Some(SafeTensorError::DuplicateName(key));
            } else if key == METADATA_KEY {
                out.metadata = Some(map.next_value::<HashMap<String, String>>()?);
            } else {
                let v = map.next_value::<serde_json::Value>()?;
                match info_from_value(&key, v) {
                    Ok(info) => out.entries.push((key, info)),
                    Err(e) => first_err = Some(e),
                }
            }
        }
        Ok(match first_err {
            Some(e) => Err(e),
            None => Ok(out),
        })
    }
}

fn parse_header(text: &str) -> Result<RawHeader, SafeTensorError> {
    let mut de = serde_json::Deserializer::from_str(text);
    let res = (&mut de)
        .deserialize_map(HeaderVisitor)
        .map_err(|e| bad(e.to_string()))?;
    let raw = res?;
    // Only trailing whitespace may follow the object.
    de.end().map_err(|e| bad(e.to_string()))?;
    Ok(raw)
}

fn validate(raw: RawHeader) -> Result<Metadata, SafeTensorError> {
    let mut entries = raw.entries;
    // Stable sort: zero-length tensors sharing an offset keep file order.
    entries.sort_by_key(|(_, i)| i.data_offsets);
    let mut cursor = 0usize;
    for (name, info) in &entries {
        let (b, e) = info.data_offsets;
        if b != cursor || e < b {
            return Err(SafeTensorError::InvalidOffset(name.clone()));
        }
        if byte_len(info.dtype, &info.shape)? != e - b {
            return Err(SafeTensorError::TensorInvalidInfo);
        }
        cursor = e;
    }
    let mut names = Vec::with_capacity(entries.len());
    let mut tensors = Vec::with_capacity(entries.len());
    let mut index_map = HashMap::with_capacity(entries.len());
    for (i, (n, t)) in entries.into_iter().enumerate() {
        index_map.insert(n.clone(), i);
        names.push(n);
        tensors.push(t);
    }
    Ok(Metadata {
        metadata: raw.metadata,
        tensors,
        names,
        index_map,
    })
}

/// Zero-copy view over a validated safetensors buffer.
#[derive(Debug, Clone)]
pub struct SafeTensors<'data> {
    metadata: Metadata,
    data: &'data [u8],
}

impl<'data> SafeTensors<'data> {
    /// Parse and validate only the header. Returns `(header_len, metadata)`.
    pub fn read_metadata(buffer: &'data [u8]) -> Result<(usize, Metadata), SafeTensorError> {
        if buffer.len() < 8 {
            return Err(SafeTensorError::HeaderTooSmall);
        }
        let mut len8 = [0u8; 8];
        len8.copy_from_slice(&buffer[..8]);
        let n = u64::from_le_bytes(len8);
        let n: usize = usize::try_from(n).map_err(|_| SafeTensorError::HeaderTooLarge)?;
        if n > MAX_HEADER_SIZE {
            return Err(SafeTensorError::HeaderTooLarge);
        }
        let stop = n
            .checked_add(8)
            .ok_or(SafeTensorError::InvalidHeaderLength)?;
        if stop > buffer.len() {
            return Err(SafeTensorError::InvalidHeaderLength);
        }
        let text = std::str::from_utf8(&buffer[8..stop])
            .map_err(|e| SafeTensorError::InvalidHeader(e.to_string()))?;
        if !text.starts_with('{') {
            return Err(SafeTensorError::InvalidHeaderStart);
        }
        let meta = validate(parse_header(text)?)?;
        if meta.data_len() != buffer.len() - stop {
            return Err(SafeTensorError::MetadataIncompleteBuffer);
        }
        Ok((n, meta))
    }

    /// Parse, validate and borrow the whole buffer.
    pub fn deserialize(buffer: &'data [u8]) -> Result<Self, SafeTensorError> {
        let (n, metadata) = Self::read_metadata(buffer)?;
        Ok(SafeTensors {
            metadata,
            data: &buffer[n + 8..],
        })
    }

    fn view(&self, i: usize) -> TensorView<'data> {
        let t = &self.metadata.tensors[i];
        TensorView {
            dtype: t.dtype,
            shape: t.shape.clone(),
            data: &self.data[t.data_offsets.0..t.data_offsets.1],
        }
    }

    /// All tensors in offset order.
    pub fn tensors(&self) -> Vec<(String, TensorView<'data>)> {
        self.iter().map(|(n, v)| (n.to_string(), v)).collect()
    }

    /// Iterate tensors in offset order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, TensorView<'data>)> {
        self.metadata
            .names
            .iter()
            .enumerate()
            .map(move |(i, n)| (n.as_str(), self.view(i)))
    }

    /// One tensor by name.
    pub fn tensor(&self, tensor_name: &str) -> Result<TensorView<'data>, SafeTensorError> {
        self.metadata
            .index_map
            .get(tensor_name)
            .map(|&i| self.view(i))
            .ok_or_else(|| SafeTensorError::TensorNotFound(tensor_name.to_string()))
    }

    /// Tensor names in offset order.
    pub fn names(&self) -> Vec<&'_ str> {
        self.metadata.names.iter().map(|s| s.as_str()).collect()
    }

    pub fn len(&self) -> usize {
        self.metadata.tensors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.metadata.tensors.is_empty()
    }

    /// The parsed header.
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
}

/// Borrowed tensor: dtype, shape and its exact bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TensorView<'data> {
    dtype: Dtype,
    shape: Vec<usize>,
    data: &'data [u8],
}

impl<'data> TensorView<'data> {
    /// Checked constructor: `data.len()` must equal dtype x shape.
    pub fn new(
        dtype: Dtype,
        shape: Vec<usize>,
        data: &'data [u8],
    ) -> Result<Self, SafeTensorError> {
        match byte_len(dtype, &shape) {
            Ok(n) if n == data.len() => Ok(TensorView { dtype, shape, data }),
            _ => Err(SafeTensorError::InvalidTensorView(dtype, shape, data.len())),
        }
    }

    pub fn dtype(&self) -> Dtype {
        self.dtype
    }

    pub fn shape(&self) -> &[usize] {
        &self.shape
    }

    pub fn data(&self) -> &'data [u8] {
        self.data
    }

    /// Chunks of bytes that make up a sub-tensor (see [`crate::slice`]).
    pub fn sliced_data(
        &'data self,
        slices: &[TensorIndexer],
    ) -> Result<SliceIterator<'data>, InvalidSlice> {
        SliceIterator::new(self, slices)
    }
}

impl View for TensorView<'_> {
    fn dtype(&self) -> Dtype {
        self.dtype
    }
    fn shape(&self) -> &[usize] {
        &self.shape
    }
    fn data(&self) -> Cow<'_, [u8]> {
        Cow::Borrowed(self.data)
    }
    fn data_len(&self) -> usize {
        self.data.len()
    }
}

impl View for &TensorView<'_> {
    fn dtype(&self) -> Dtype {
        self.dtype
    }
    fn shape(&self) -> &[usize] {
        &self.shape
    }
    fn data(&self) -> Cow<'_, [u8]> {
        Cow::Borrowed(self.data)
    }
    fn data_len(&self) -> usize {
        self.data.len()
    }
}

/// Sorted tensors + the finished header (length prefix + padded JSON).
fn prepare<S, V, I>(
    data: I,
    data_info: Option<HashMap<String, String>>,
) -> Result<(Vec<u8>, Vec<V>, usize), SafeTensorError>
where
    S: AsRef<str> + Ord + fmt::Display,
    V: View,
    I: IntoIterator<Item = (S, V)>,
{
    let mut items: Vec<(S, V)> = data.into_iter().collect();
    // Highest alignment first, then name: keeps every tensor naturally aligned after the 8-aligned header.
    items.sort_by(|(ln, lv), (rn, rv)| rv.dtype().cmp(&lv.dtype()).then(ln.cmp(rn)));

    let mut json = String::from("{");
    let mut first = true;
    if let Some(info) = &data_info {
        let sorted: BTreeMap<&String, &String> = info.iter().collect();
        json.push_str("\"__metadata__\":");
        json.push_str(&serde_json::to_string(&sorted).map_err(|e| bad(e.to_string()))?);
        first = false;
    }
    let mut seen = HashSet::new();
    let mut offset = 0usize;
    let mut views = Vec::with_capacity(items.len());
    for (name, view) in items {
        let name = name.as_ref();
        if name == METADATA_KEY || !seen.insert(name.to_string()) {
            return Err(SafeTensorError::DuplicateName(name.to_string()));
        }
        let want = byte_len(view.dtype(), view.shape())?;
        if want != view.data_len() {
            return Err(SafeTensorError::InvalidTensorView(
                view.dtype(),
                view.shape().to_vec(),
                view.data_len(),
            ));
        }
        let end = offset
            .checked_add(want)
            .ok_or(SafeTensorError::ValidationOverflow)?;
        if !first {
            json.push(',');
        }
        first = false;
        json.push_str(&serde_json::to_string(name).map_err(|e| bad(e.to_string()))?);
        json.push_str(":{\"dtype\":\"");
        json.push_str(view.dtype().as_str());
        json.push_str("\",\"shape\":[");
        let dims: Vec<String> = view.shape().iter().map(|d| d.to_string()).collect();
        json.push_str(&dims.join(","));
        json.push_str(&format!("],\"data_offsets\":[{offset},{end}]}}"));
        offset = end;
        views.push(view);
    }
    json.push('}');
    while !json.len().is_multiple_of(8) {
        json.push(' ');
    }
    if json.len() > MAX_HEADER_SIZE {
        return Err(SafeTensorError::HeaderTooLarge);
    }
    let mut header = Vec::with_capacity(8 + json.len());
    header.extend_from_slice(&(json.len() as u64).to_le_bytes());
    header.extend_from_slice(json.as_bytes());
    Ok((header, views, offset))
}

/// Serialize tensors (+ optional `__metadata__`) into one in-memory buffer.
pub fn serialize<S: AsRef<str> + Ord + fmt::Display, V: View, I: IntoIterator<Item = (S, V)>>(
    data: I,
    data_info: Option<HashMap<String, String>>,
) -> Result<Vec<u8>, SafeTensorError> {
    let (header, views, total) = prepare(data, data_info)?;
    let mut out = Vec::with_capacity(header.len() + total);
    out.extend_from_slice(&header);
    for v in &views {
        let bytes = v.data();
        if bytes.len() != v.data_len() {
            return Err(SafeTensorError::InvalidTensorView(
                v.dtype(),
                v.shape().to_vec(),
                bytes.len(),
            ));
        }
        out.extend_from_slice(&bytes);
    }
    Ok(out)
}

/// Serialize tensors straight to `filename` (buffered, one tensor in memory at a time).
pub fn serialize_to_file<S, V, I>(
    data: I,
    data_info: Option<HashMap<String, String>>,
    filename: &Path,
) -> Result<(), SafeTensorError>
where
    S: AsRef<str> + Ord + fmt::Display,
    V: View,
    I: IntoIterator<Item = (S, V)>,
{
    let (header, views, _) = prepare(data, data_info)?;
    let mut f = std::io::BufWriter::new(std::fs::File::create(filename)?);
    f.write_all(&header)?;
    for v in &views {
        let bytes = v.data();
        if bytes.len() != v.data_len() {
            return Err(SafeTensorError::InvalidTensorView(
                v.dtype(),
                v.shape().to_vec(),
                bytes.len(),
            ));
        }
        f.write_all(&bytes)?;
    }
    f.flush()?;
    Ok(())
}
