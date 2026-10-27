// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Format, validation, hostile-input and slicing tests for the clean-room safetensors reader.

use safetensors::slice::{IndexOp, InvalidSlice, TensorIndexer};
use safetensors::tensor::{Dtype, SafeTensorError, SafeTensors, TensorView};
use safetensors::{serialize, serialize_to_file};
use std::collections::HashMap;
use std::ops::Bound;

fn raw(header: &str, data: &[u8]) -> Vec<u8> {
    let mut v = (header.len() as u64).to_le_bytes().to_vec();
    v.extend_from_slice(header.as_bytes());
    v.extend_from_slice(data);
    v
}

fn f32s(n: usize) -> Vec<u8> {
    (0..n).flat_map(|i| (i as f32).to_le_bytes()).collect()
}

#[test]
fn round_trip_with_metadata_and_alignment() {
    let a = f32s(6);
    let b: Vec<u8> = (0u8..3).collect();
    let c: Vec<u8> = (0..4u64).flat_map(|i| i.to_le_bytes()).collect();
    let va = TensorView::new(Dtype::F32, vec![2, 3], &a).unwrap();
    let vb = TensorView::new(Dtype::U8, vec![3], &b).unwrap();
    let vc = TensorView::new(Dtype::I64, vec![4], &c).unwrap();
    let mut meta = HashMap::new();
    meta.insert("format".to_string(), "pt".to_string());
    meta.insert("a\"quote".to_string(), "line\nbreak".to_string());
    let bytes = serialize(
        [("weight", &va), ("bias", &vb), ("ids", &vc)],
        Some(meta.clone()),
    )
    .unwrap();
    // Header length is a multiple of 8, so the data buffer starts 8-aligned.
    let n = u64::from_le_bytes(bytes[..8].try_into().unwrap()) as usize;
    assert_eq!(n % 8, 0);
    let st = SafeTensors::deserialize(&bytes).unwrap();
    assert_eq!(st.len(), 3);
    // Highest alignment first: I64, F32, U8.
    assert_eq!(st.names(), vec!["ids", "weight", "bias"]);
    assert_eq!(st.tensor("weight").unwrap(), va);
    assert_eq!(st.tensor("bias").unwrap().data(), &b[..]);
    assert_eq!(st.tensor("ids").unwrap().shape(), &[4]);
    assert_eq!(st.metadata().metadata().as_ref().unwrap(), &meta);
    assert!(matches!(
        st.tensor("nope"),
        Err(SafeTensorError::TensorNotFound(_))
    ));
    // Deterministic: same input -> same bytes.
    let mut meta2 = HashMap::new();
    meta2.insert("a\"quote".to_string(), "line\nbreak".to_string());
    meta2.insert("format".to_string(), "pt".to_string());
    let again = serialize([("bias", &vb), ("ids", &vc), ("weight", &va)], Some(meta2)).unwrap();
    assert_eq!(bytes, again);
}

#[test]
fn exact_wire_header() {
    let a = f32s(2);
    let v = TensorView::new(Dtype::F32, vec![2], &a).unwrap();
    let bytes = serialize([("x", v)], None).unwrap();
    let n = u64::from_le_bytes(bytes[..8].try_into().unwrap()) as usize;
    let hdr = std::str::from_utf8(&bytes[8..8 + n]).unwrap();
    assert_eq!(
        hdr.trim_end(),
        r#"{"x":{"dtype":"F32","shape":[2],"data_offsets":[0,8]}}"#
    );
}

#[test]
fn file_round_trip() {
    let dir = std::env::temp_dir().join(format!("st-mit-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("t.safetensors");
    let a = f32s(12);
    let v = TensorView::new(Dtype::F32, vec![3, 4], &a).unwrap();
    serialize_to_file([("t", &v)], None, &p).unwrap();
    let bytes = std::fs::read(&p).unwrap();
    assert_eq!(bytes, serialize([("t", &v)], None).unwrap());
    let st = SafeTensors::deserialize(&bytes).unwrap();
    assert_eq!(st.tensor("t").unwrap().data(), &a[..]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn reads_hand_written_file_with_padding_and_empty_tensor() {
    let h = r#"{"__metadata__":{"k":"v"},"e":{"dtype":"F16","shape":[0,3],"data_offsets":[0,0]},"a":{"dtype":"BF16","shape":[2],"data_offsets":[0,4]}}      "#;
    let f = raw(h, &[1, 2, 3, 4]);
    let st = SafeTensors::deserialize(&f).unwrap();
    assert_eq!(st.tensor("e").unwrap().data().len(), 0);
    assert_eq!(st.tensor("a").unwrap().data(), &[1, 2, 3, 4]);
    assert_eq!(st.metadata().data_len(), 4);
}

fn err(h: &str, data: &[u8]) -> SafeTensorError {
    SafeTensors::deserialize(&raw(h, data)).unwrap_err()
}

#[test]
fn rejects_hostile_headers() {
    use SafeTensorError::*;
    assert!(matches!(
        SafeTensors::deserialize(&[1, 2, 3]),
        Err(HeaderTooSmall)
    ));
    let mut huge = u64::MAX.to_le_bytes().to_vec();
    huge.extend_from_slice(b"{}");
    assert!(matches!(
        SafeTensors::deserialize(&huge),
        Err(HeaderTooLarge)
    ));
    let mut long = 50u64.to_le_bytes().to_vec();
    long.extend_from_slice(b"{}");
    assert!(matches!(
        SafeTensors::deserialize(&long),
        Err(InvalidHeaderLength)
    ));
    let mut bad_utf8 = 2u64.to_le_bytes().to_vec();
    bad_utf8.extend_from_slice(&[0xff, 0xfe]);
    assert!(matches!(
        SafeTensors::deserialize(&bad_utf8),
        Err(InvalidHeader(_))
    ));
    assert!(matches!(err(" {}", &[]), InvalidHeaderStart));
    assert!(matches!(err("[]", &[]), InvalidHeaderStart));
    assert!(matches!(err("{}x", &[]), InvalidHeaderDeserialization(_)));
    assert!(matches!(err("{", &[]), InvalidHeaderDeserialization(_)));
    // Duplicate key: a parser differential (first vs last wins) must not be possible.
    let dup = r#"{"a":{"dtype":"U8","shape":[1],"data_offsets":[0,1]},"a":{"dtype":"U8","shape":[1],"data_offsets":[0,1]}}"#;
    assert!(matches!(err(dup, &[0]), DuplicateName(_)));
    let dupm = r#"{"__metadata__":{},"__metadata__":{}}"#;
    assert!(matches!(err(dupm, &[]), DuplicateName(_)));
    // Gap at start, overlap, wrong size, short and long buffers.
    let gap = r#"{"a":{"dtype":"U8","shape":[1],"data_offsets":[1,2]}}"#;
    assert!(matches!(err(gap, &[0, 0]), InvalidOffset(_)));
    let overlap = r#"{"a":{"dtype":"U8","shape":[2],"data_offsets":[0,2]},"b":{"dtype":"U8","shape":[2],"data_offsets":[1,3]}}"#;
    assert!(matches!(err(overlap, &[0, 0, 0]), InvalidOffset(_)));
    let reversed = r#"{"a":{"dtype":"U8","shape":[0],"data_offsets":[2,0]}}"#;
    assert!(matches!(err(reversed, &[0, 0]), InvalidOffset(_)));
    let size = r#"{"a":{"dtype":"F32","shape":[2],"data_offsets":[0,4]}}"#;
    assert!(matches!(err(size, &[0; 4]), TensorInvalidInfo));
    let ok = r#"{"a":{"dtype":"U8","shape":[2],"data_offsets":[0,2]}}"#;
    assert!(matches!(err(ok, &[0; 3]), MetadataIncompleteBuffer));
    assert!(matches!(err(ok, &[0; 1]), MetadataIncompleteBuffer));
    // Overflowing shape.
    let ovf = format!(
        r#"{{"a":{{"dtype":"F64","shape":[{},{}],"data_offsets":[0,0]}}}}"#,
        u64::MAX,
        4
    );
    assert!(matches!(
        err(&ovf, &[]),
        ValidationOverflow | InvalidHeaderDeserialization(_)
    ));
    // Sub-byte dtype with odd element count.
    let f4 = r#"{"a":{"dtype":"F4","shape":[3],"data_offsets":[0,1]}}"#;
    assert!(matches!(err(f4, &[0]), MisalignedSlice));
    // Schema errors.
    for h in [
        r#"{"a":{"dtype":"Q8","shape":[1],"data_offsets":[0,1]}}"#,
        r#"{"a":{"dtype":"u8","shape":[1],"data_offsets":[0,1]}}"#,
        r#"{"a":{"dtype":"U8","shape":[-1],"data_offsets":[0,1]}}"#,
        r#"{"a":{"dtype":"U8","shape":[1.5],"data_offsets":[0,1]}}"#,
        r#"{"a":{"dtype":"U8","shape":[1],"data_offsets":[0]}}"#,
        r#"{"a":{"dtype":"U8","shape":[1]}}"#,
        r#"{"a":[1,2]}"#,
        r#"{"__metadata__":{"k":1}}"#,
        r#"{"__metadata__":[]}"#,
    ] {
        assert!(
            matches!(err(h, &[0]), InvalidHeaderDeserialization(_)),
            "{h}"
        );
    }
}

#[test]
fn serialize_rejects_bad_input() {
    let d = [0u8; 4];
    let v = TensorView::new(Dtype::U8, vec![4], &d).unwrap();
    assert!(matches!(
        serialize([("a", &v), ("a", &v)], None),
        Err(SafeTensorError::DuplicateName(_))
    ));
    assert!(matches!(
        serialize([("__metadata__", &v)], None),
        Err(SafeTensorError::DuplicateName(_))
    ));
    assert!(matches!(
        TensorView::new(Dtype::F32, vec![2], &d),
        Err(SafeTensorError::InvalidTensorView(..))
    ));
}

/// Reference: copy out a sub-tensor element by element.
fn naive(data: &[u8], shape: &[usize], es: usize, r: &[(usize, usize)]) -> Vec<u8> {
    let mut out = Vec::new();
    let n: usize = r.iter().map(|(a, b)| b - a).product();
    if n == 0 {
        return out;
    }
    let mut idx: Vec<usize> = r.iter().map(|x| x.0).collect();
    loop {
        let mut off = 0;
        for (i, &x) in idx.iter().enumerate() {
            off = off * shape[i] + x;
        }
        out.extend_from_slice(&data[off * es..(off + 1) * es]);
        let mut i = idx.len();
        loop {
            if i == 0 {
                return out;
            }
            i -= 1;
            idx[i] += 1;
            if idx[i] < r[i].1 {
                break;
            }
            idx[i] = r[i].0;
        }
    }
}

#[test]
fn slices_match_naive_reference() {
    let shape = [3usize, 4, 5];
    let data = f32s(60);
    let v = TensorView::new(Dtype::F32, shape.to_vec(), &data).unwrap();
    let mut seed = 0x2545F4914F6CDD1Du64;
    let mut rnd = |m: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % (m as u64 + 1)) as usize
    };
    for _ in 0..500 {
        let mut idx = Vec::new();
        let mut r = Vec::new();
        let mut newshape = Vec::new();
        let ndims = rnd(3);
        for (d, &s) in shape.iter().enumerate() {
            if d >= ndims {
                r.push((0, s));
                newshape.push(s);
                continue;
            }
            if rnd(3) == 0 {
                let j = rnd(s - 1);
                idx.push(TensorIndexer::Select(j));
                r.push((j, j + 1));
            } else {
                let a = rnd(s);
                let b = a + rnd(s - a);
                idx.push(TensorIndexer::Narrow(
                    Bound::Included(a),
                    Bound::Excluded(b),
                ));
                r.push((a, b));
                newshape.push(b - a);
            }
        }
        let it = v.sliced_data(&idx).unwrap();
        assert_eq!(it.newshape(), newshape);
        let want = naive(&data, &shape, 4, &r);
        assert_eq!(it.remaining_byte_len(), want.len());
        let got: Vec<u8> = it.flatten().copied().collect();
        assert_eq!(got, want, "{idx:?}");
    }
}

#[test]
fn index_op_forms_used_by_candle_nn() {
    let data = f32s(12);
    let v = TensorView::new(Dtype::F32, vec![3, 4], &data).unwrap();
    // candle-nn ShardedSafeTensors: dim 0 -> view.slice(start..stop), dim 1 -> view.slice((.., start..stop)).
    let rows: Vec<u8> = v.slice(1..3).unwrap().flatten().copied().collect();
    assert_eq!(rows, data[16..48].to_vec());
    let cols: Vec<u8> = v.slice((.., 1..3)).unwrap().flatten().copied().collect();
    assert_eq!(cols, naive(&data, &[3, 4], 4, &[(0, 3), (1, 3)]));
    assert_eq!(v.slice((.., 1..3)).unwrap().count(), 3);
    assert_eq!(v.slice(..).unwrap().count(), 1);
    assert_eq!(v.slice(2usize).unwrap().newshape(), vec![4]);
    assert_eq!(v.slice((0..=1, ..2)).unwrap().newshape(), vec![2, 2]);
    assert!(matches!(
        v.slice((0, 0, 0)),
        Err(InvalidSlice::TooManySlices)
    ));
    assert!(matches!(
        v.slice(3usize),
        Err(InvalidSlice::SliceOutOfRange { dim_index: 0, .. })
    ));
    assert!(matches!(
        v.slice(0..5),
        Err(InvalidSlice::SliceOutOfRange { .. })
    ));
    assert_eq!(v.slice(2..2).unwrap().count(), 0);
}

#[test]
fn sub_byte_slicing() {
    let d = [0x21u8, 0x43, 0x65, 0x87];
    let v = TensorView::new(Dtype::F4, vec![2, 4], &d).unwrap();
    let row: Vec<u8> = v.slice(1..2).unwrap().flatten().copied().collect();
    assert_eq!(row, vec![0x65, 0x87]);
    assert!(matches!(
        v.slice((.., 1..2)),
        Err(InvalidSlice::MisalignedSlice)
    ));
}

#[test]
fn mutated_inputs_never_panic() {
    let a = f32s(16);
    let b: Vec<u8> = (0..8).collect();
    let va = TensorView::new(Dtype::F32, vec![4, 4], &a).unwrap();
    let vb = TensorView::new(Dtype::U8, vec![8], &b).unwrap();
    let good = serialize([("a", &va), ("b", &vb)], None).unwrap();
    let mut seed = 0x9E3779B97F4A7C15u64;
    let mut ok = 0;
    for _ in 0..20_000 {
        let mut m = good.clone();
        for _ in 0..1 + (seed % 4) as usize {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let i = (seed as usize) % m.len();
            m[i] = (seed >> 32) as u8;
        }
        if seed.is_multiple_of(7) {
            m.truncate((seed as usize >> 8) % m.len());
        }
        if let Ok(st) = SafeTensors::deserialize(&m) {
            ok += 1;
            for (_, v) in st.tensors() {
                let _ = v.slice(..).map(|it| it.count());
            }
        }
    }
    // Mutations that hit only tensor data keep the file valid.
    assert!(ok > 0);
}
