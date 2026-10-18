// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Slavik / Galion Studio

//! Speaker verification: voice as a SOFT identity signal.
//!
//! * Embeddings come from a speaker model (e.g. ECAPA-TDNN or WeSpeaker
//!   ResNet34 exported to ONNX, run on the VPS CPU). The model runtime is
//!   behind the [`SpeakerVerifier`] trait; this crate ships no model.
//! * Score = cosine similarity between the live embedding and the enrolled
//!   voiceprint (L2-normalised centroid of several enrollment phrases).
//! * Two thresholds give three outcomes: accept / uncertain / reject.
//!   Values are config placeholders until measured on real audio.
//! * Voice is NEVER sufficient on its own for privileged actions, and money is
//!   never authorised at all (see [`authorize`]). The stack itself clones
//!   voices (Chatterbox, PersonaPlex voice prompts), so voice is spoofable.

use std::fmt;

/// A speaker embedding (not necessarily normalised).
#[derive(Debug, Clone, PartialEq)]
pub struct Embedding(pub Vec<f32>);

/// Cosine similarity in [-1, 1]. `None` if lengths differ, either vector is
/// empty, has zero norm, or contains a non-finite value.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> Option<f32> {
    if a.len() != b.len() || a.is_empty() {
        return None;
    }
    let (mut dot, mut na, mut nb) = (0.0f64, 0.0f64, 0.0f64);
    for (&x, &y) in a.iter().zip(b) {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let (x, y) = (f64::from(x), f64::from(y));
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        return None;
    }
    Some((dot / (na.sqrt() * nb.sqrt())).clamp(-1.0, 1.0) as f32)
}

/// L2-normalise. `None` for empty, zero or non-finite input.
pub fn l2_normalize(v: &[f32]) -> Option<Vec<f32>> {
    if v.is_empty() || v.iter().any(|x| !x.is_finite()) {
        return None;
    }
    let n = v
        .iter()
        .map(|&x| f64::from(x) * f64::from(x))
        .sum::<f64>()
        .sqrt();
    if n == 0.0 {
        return None;
    }
    Some(v.iter().map(|&x| (f64::from(x) / n) as f32).collect())
}

/// Accept / reject cosine thresholds. `reject <= accept`; scores in between
/// are "uncertain". Raising `accept` lowers false accepts and raises false
/// rejects - that is what "very sensitive" trades.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thresholds {
    accept: f32,
    reject: f32,
}

impl Thresholds {
    pub fn new(accept: f32, reject: f32) -> Option<Self> {
        let ok = |x: f32| x.is_finite() && (-1.0..=1.0).contains(&x);
        (ok(accept) && ok(reject) && reject <= accept).then_some(Self { accept, reject })
    }
    pub fn accept(&self) -> f32 {
        self.accept
    }
    pub fn reject(&self) -> f32 {
        self.reject
    }
}

/// Outcome of one verification window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Decision {
    Accept = 0,
    Uncertain = 1,
    Reject = 2,
    /// Not enough voiced speech in the window to score.
    InsufficientAudio = 3,
}

/// Anti-spoofing / liveness status for the same window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    NotChecked,
    Passed,
    /// Countermeasure flagged replay or synthetic speech.
    Failed,
}

/// Per-window result reported on the control band.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VerificationResult {
    pub score: Option<f32>,
    pub decision: Decision,
    pub liveness: Liveness,
    pub speech_ms: u32,
}

/// Threshold logic. A failed liveness check always forces Reject.
pub fn decide(
    score: Option<f32>,
    speech_ms: u32,
    min_speech_ms: u32,
    t: Thresholds,
    liveness: Liveness,
) -> Decision {
    if liveness == Liveness::Failed {
        return Decision::Reject;
    }
    if speech_ms < min_speech_ms {
        return Decision::InsufficientAudio;
    }
    match score {
        None => Decision::InsufficientAudio,
        Some(s) if s >= t.accept => Decision::Accept,
        Some(s) if s < t.reject => Decision::Reject,
        Some(_) => Decision::Uncertain,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum VerifyError {
    /// Model missing or failed to load.
    ModelUnavailable(String),
    DimensionMismatch {
        expected: usize,
        got: usize,
    },
    BadAudio(String),
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerifyError::ModelUnavailable(m) => write!(f, "speaker model unavailable: {m}"),
            VerifyError::DimensionMismatch { expected, got } => {
                write!(f, "embedding dim {got}, expected {expected}")
            }
            VerifyError::BadAudio(m) => write!(f, "bad audio: {m}"),
        }
    }
}

impl std::error::Error for VerifyError {}

/// Enrolled voiceprint: unit-length centroid of the enrollment embeddings.
/// This is biometric data (GDPR art. 9): store encrypted, box-local, deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct VoicePrint {
    centroid: Vec<f32>,
    phrases: u32,
}

impl VoicePrint {
    pub fn centroid(&self) -> &[f32] {
        &self.centroid
    }
    pub fn phrases(&self) -> u32 {
        self.phrases
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum EnrollError {
    TooFewPhrases {
        need: u32,
        got: u32,
    },
    BadEmbedding(usize),
    DimensionMismatch(usize),
    /// Phrase `index` disagrees with the rest (wrong speaker, noise, or replay).
    Inconsistent {
        index: usize,
        score: f32,
    },
}

/// Build a voiceprint from several enrollment phrases. Each embedding is
/// normalised, averaged, re-normalised; then every phrase must agree with the
/// centroid by at least `min_consistency`.
pub fn enroll(
    embeddings: &[Embedding],
    min_phrases: u32,
    min_consistency: f32,
) -> Result<VoicePrint, EnrollError> {
    let got = u32::try_from(embeddings.len()).unwrap_or(u32::MAX);
    if got < min_phrases {
        return Err(EnrollError::TooFewPhrases {
            need: min_phrases,
            got,
        });
    }
    let dim = embeddings[0].0.len();
    let mut sum = vec![0.0f64; dim];
    let mut normed = Vec::with_capacity(embeddings.len());
    for (i, e) in embeddings.iter().enumerate() {
        if e.0.len() != dim {
            return Err(EnrollError::DimensionMismatch(i));
        }
        let n = l2_normalize(&e.0).ok_or(EnrollError::BadEmbedding(i))?;
        for (s, &x) in sum.iter_mut().zip(&n) {
            *s += f64::from(x);
        }
        normed.push(n);
    }
    let mean: Vec<f32> = sum.iter().map(|&s| s as f32).collect();
    let centroid = l2_normalize(&mean).ok_or(EnrollError::BadEmbedding(0))?;
    for (i, n) in normed.iter().enumerate() {
        let score = cosine_similarity(n, &centroid).unwrap_or(-1.0);
        if score < min_consistency {
            return Err(EnrollError::Inconsistent { index: i, score });
        }
    }
    Ok(VoicePrint {
        centroid,
        phrases: got,
    })
}

/// Verification policy taken from `[speaker]` config.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Policy {
    pub thresholds: Thresholds,
    pub min_speech_ms: u32,
    pub require_liveness: bool,
}

/// A speaker model runtime (ONNX on CPU in the real implementation).
pub trait SpeakerVerifier {
    /// Length of embeddings this model produces.
    fn embedding_dim(&self) -> usize;

    /// Embed mono PCM at 16 kHz (most speaker models expect 16 kHz; the relay
    /// resamples from Mimi's 24 kHz before calling this).
    fn embed(&self, pcm_16k: &[f32]) -> Result<Embedding, VerifyError>;

    /// Anti-spoofing countermeasure. Default: not checked.
    fn liveness(&self, _pcm_16k: &[f32]) -> Liveness {
        Liveness::NotChecked
    }

    /// Score one window against a voiceprint and apply the policy.
    fn verify(
        &self,
        print: &VoicePrint,
        pcm_16k: &[f32],
        speech_ms: u32,
        policy: &Policy,
    ) -> Result<VerificationResult, VerifyError> {
        let liveness = self.liveness(pcm_16k);
        if speech_ms < policy.min_speech_ms {
            return Ok(VerificationResult {
                score: None,
                decision: Decision::InsufficientAudio,
                liveness,
                speech_ms,
            });
        }
        let e = self.embed(pcm_16k)?;
        if e.0.len() != print.centroid.len() {
            return Err(VerifyError::DimensionMismatch {
                expected: print.centroid.len(),
                got: e.0.len(),
            });
        }
        let score = cosine_similarity(&e.0, &print.centroid);
        let mut decision = decide(
            score,
            speech_ms,
            policy.min_speech_ms,
            policy.thresholds,
            liveness,
        );
        // Without a passed liveness check an accept is downgraded to uncertain.
        if policy.require_liveness && liveness != Liveness::Passed && decision == Decision::Accept {
            decision = Decision::Uncertain;
        }
        Ok(VerificationResult {
            score,
            decision,
            liveness,
            speech_ms,
        })
    }
}

/// Identity level held by a live session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityState {
    /// No window scored yet, or only insufficient/uncertain windows.
    Unverified,
    /// Latest scored window accepted (and liveness passed if required).
    Verified,
    /// Too many consecutive rejects: identity lost for the rest of the session.
    Lost,
}

/// Continuous verification over a call: every window updates the state.
/// Once `Lost`, the session never regains `Verified` (re-auth needed).
#[derive(Debug, Clone)]
pub struct ContinuousVerifier {
    max_consecutive_rejects: u32,
    consecutive_rejects: u32,
    state: IdentityState,
}

impl ContinuousVerifier {
    pub fn new(max_consecutive_rejects: u32) -> Self {
        Self {
            max_consecutive_rejects: max_consecutive_rejects.max(1),
            consecutive_rejects: 0,
            state: IdentityState::Unverified,
        }
    }

    pub fn state(&self) -> IdentityState {
        self.state
    }

    pub fn observe(&mut self, r: &VerificationResult) -> IdentityState {
        if self.state == IdentityState::Lost {
            return self.state;
        }
        match r.decision {
            Decision::Accept => {
                self.consecutive_rejects = 0;
                self.state = IdentityState::Verified;
            }
            Decision::Reject => {
                self.consecutive_rejects += 1;
                self.state = if self.consecutive_rejects >= self.max_consecutive_rejects {
                    IdentityState::Lost
                } else {
                    IdentityState::Unverified
                };
            }
            // Uncertain drops verification but does not count toward Lost.
            Decision::Uncertain => self.state = IdentityState::Unverified,
            // Silence / short speech keeps the previous state.
            Decision::InsufficientAudio => {}
        }
        self.state
    }
}

/// What the caller wants to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionClass {
    /// Talk to Persona Plex. Voice identity not required.
    Conversation,
    /// Personalised but harmless (e.g. "read my notes"). Voice is enough.
    Personal,
    /// Admin, secrets, config, running anything. Voice + a second factor.
    Privileged,
    /// Payments, Revolut, any real money. NOT approved: always denied.
    Money,
}

/// The security stance in one function: voice is a soft signal.
pub fn authorize(action: ActionClass, identity: IdentityState, second_factor_ok: bool) -> bool {
    match action {
        ActionClass::Conversation => true,
        ActionClass::Personal => identity == IdentityState::Verified,
        ActionClass::Privileged => identity == IdentityState::Verified && second_factor_ok,
        ActionClass::Money => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t() -> Thresholds {
        Thresholds::new(0.7, 0.5).unwrap()
    }

    /// Test double: returns a fixed embedding and a fixed liveness verdict.
    struct Fixed {
        emb: Vec<f32>,
        live: Liveness,
    }

    impl SpeakerVerifier for Fixed {
        fn embedding_dim(&self) -> usize {
            self.emb.len()
        }
        fn embed(&self, _pcm: &[f32]) -> Result<Embedding, VerifyError> {
            Ok(Embedding(self.emb.clone()))
        }
        fn liveness(&self, _pcm: &[f32]) -> Liveness {
            self.live
        }
    }

    fn policy(require_liveness: bool) -> Policy {
        Policy {
            thresholds: t(),
            min_speech_ms: 1500,
            require_liveness,
        }
    }

    #[test]
    fn cosine_basics() {
        assert!((cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]).unwrap() - 1.0).abs() < 1e-6);
        assert!(cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]).unwrap().abs() < 1e-6);
        assert!((cosine_similarity(&[1.0, 2.0], &[-1.0, -2.0]).unwrap() + 1.0).abs() < 1e-6);
        // Scale invariant.
        let a = cosine_similarity(&[1.0, 2.0, 3.0], &[2.0, 4.1, 5.9]).unwrap();
        let b = cosine_similarity(&[10.0, 20.0, 30.0], &[2.0, 4.1, 5.9]).unwrap();
        assert!((a - b).abs() < 1e-6);
    }

    #[test]
    fn cosine_rejects_bad_input() {
        assert_eq!(cosine_similarity(&[], &[]), None);
        assert_eq!(cosine_similarity(&[1.0], &[1.0, 2.0]), None);
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 2.0]), None);
        assert_eq!(cosine_similarity(&[f32::NAN, 1.0], &[1.0, 2.0]), None);
        assert_eq!(cosine_similarity(&[f32::INFINITY, 1.0], &[1.0, 2.0]), None);
    }

    #[test]
    fn thresholds_validation() {
        assert!(Thresholds::new(0.7, 0.5).is_some());
        assert!(Thresholds::new(0.6, 0.6).is_some());
        assert!(Thresholds::new(0.5, 0.7).is_none());
        assert!(Thresholds::new(1.5, 0.5).is_none());
        assert!(Thresholds::new(f32::NAN, 0.5).is_none());
    }

    #[test]
    fn threshold_bands() {
        let d = |s| decide(Some(s), 2000, 1500, t(), Liveness::NotChecked);
        assert_eq!(d(0.70), Decision::Accept, "accept is inclusive");
        assert_eq!(d(0.95), Decision::Accept);
        assert_eq!(d(0.69), Decision::Uncertain);
        assert_eq!(d(0.50), Decision::Uncertain, "reject is exclusive");
        assert_eq!(d(0.49), Decision::Reject);
        assert_eq!(d(-0.3), Decision::Reject);
        assert_eq!(
            decide(Some(0.9), 1000, 1500, t(), Liveness::NotChecked),
            Decision::InsufficientAudio
        );
        assert_eq!(
            decide(None, 2000, 1500, t(), Liveness::NotChecked),
            Decision::InsufficientAudio
        );
        assert_eq!(
            decide(Some(0.99), 2000, 1500, t(), Liveness::Failed),
            Decision::Reject
        );
    }

    #[test]
    fn enrollment() {
        let e = |v: &[f32]| Embedding(v.to_vec());
        let good = [
            e(&[1.0, 0.1, 0.0]),
            e(&[0.9, 0.0, 0.1]),
            e(&[1.1, 0.05, 0.05]),
        ];
        let vp = enroll(&good, 3, 0.9).unwrap();
        assert_eq!(vp.phrases(), 3);
        let norm: f32 = vp.centroid().iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5);

        assert!(matches!(
            enroll(&good[..2], 3, 0.9),
            Err(EnrollError::TooFewPhrases { need: 3, got: 2 })
        ));
        let mixed = [
            e(&[1.0, 0.0, 0.0]),
            e(&[1.0, 0.1, 0.0]),
            e(&[0.0, 0.0, 1.0]),
        ];
        assert!(matches!(
            enroll(&mixed, 3, 0.8),
            Err(EnrollError::Inconsistent { index: 2, .. })
        ));
        let zero = [
            e(&[1.0, 0.0, 0.0]),
            e(&[0.0, 0.0, 0.0]),
            e(&[1.0, 0.0, 0.0]),
        ];
        assert_eq!(enroll(&zero, 3, 0.5), Err(EnrollError::BadEmbedding(1)));
        let dims = [e(&[1.0, 0.0, 0.0]), e(&[1.0, 0.0]), e(&[1.0, 0.0, 0.0])];
        assert_eq!(
            enroll(&dims, 3, 0.5),
            Err(EnrollError::DimensionMismatch(1))
        );
    }

    #[test]
    fn verify_through_trait() {
        let one = Embedding(vec![1.0, 0.0]);
        let vp = enroll(&[one.clone(), one.clone(), one], 3, 0.9).unwrap();
        let same = Fixed {
            emb: vec![0.98, 0.05],
            live: Liveness::Passed,
        };
        let r = same.verify(&vp, &[0.0; 16], 2000, &policy(true)).unwrap();
        assert_eq!(r.decision, Decision::Accept);
        assert!(r.score.unwrap() > 0.99);

        let other = Fixed {
            emb: vec![0.0, 1.0],
            live: Liveness::Passed,
        };
        assert_eq!(
            other
                .verify(&vp, &[], 2000, &policy(true))
                .unwrap()
                .decision,
            Decision::Reject
        );

        // Liveness required but not checked: accept is downgraded.
        let unchecked = Fixed {
            emb: vec![1.0, 0.0],
            live: Liveness::NotChecked,
        };
        assert_eq!(
            unchecked
                .verify(&vp, &[], 2000, &policy(true))
                .unwrap()
                .decision,
            Decision::Uncertain
        );
        assert_eq!(
            unchecked
                .verify(&vp, &[], 2000, &policy(false))
                .unwrap()
                .decision,
            Decision::Accept
        );

        // A perfect clone that fails liveness is rejected.
        let clone = Fixed {
            emb: vec![1.0, 0.0],
            live: Liveness::Failed,
        };
        assert_eq!(
            clone
                .verify(&vp, &[], 2000, &policy(true))
                .unwrap()
                .decision,
            Decision::Reject
        );

        // Short speech: no embedding computed, no score.
        let r = same.verify(&vp, &[], 500, &policy(true)).unwrap();
        assert_eq!((r.score, r.decision), (None, Decision::InsufficientAudio));

        let wrong_dim = Fixed {
            emb: vec![1.0, 0.0, 0.0],
            live: Liveness::Passed,
        };
        assert!(matches!(
            wrong_dim.verify(&vp, &[], 2000, &policy(true)),
            Err(VerifyError::DimensionMismatch {
                expected: 2,
                got: 3
            })
        ));
    }

    fn res(decision: Decision) -> VerificationResult {
        VerificationResult {
            score: None,
            decision,
            liveness: Liveness::Passed,
            speech_ms: 2000,
        }
    }

    #[test]
    fn continuous_verification() {
        let mut cv = ContinuousVerifier::new(2);
        assert_eq!(cv.state(), IdentityState::Unverified);
        assert_eq!(cv.observe(&res(Decision::Accept)), IdentityState::Verified);
        assert_eq!(
            cv.observe(&res(Decision::InsufficientAudio)),
            IdentityState::Verified
        );
        assert_eq!(
            cv.observe(&res(Decision::Uncertain)),
            IdentityState::Unverified
        );
        assert_eq!(
            cv.observe(&res(Decision::Reject)),
            IdentityState::Unverified
        );
        assert_eq!(
            cv.observe(&res(Decision::Accept)),
            IdentityState::Verified,
            "accept resets count"
        );
        assert_eq!(
            cv.observe(&res(Decision::Reject)),
            IdentityState::Unverified
        );
        assert_eq!(cv.observe(&res(Decision::Reject)), IdentityState::Lost);
        assert_eq!(
            cv.observe(&res(Decision::Accept)),
            IdentityState::Lost,
            "lost is sticky"
        );
    }

    #[test]
    fn voice_is_never_the_only_key() {
        use ActionClass::*;
        use IdentityState::*;
        assert!(authorize(Conversation, Unverified, false));
        assert!(authorize(Personal, Verified, false));
        assert!(!authorize(Personal, Unverified, false));
        assert!(
            !authorize(Privileged, Verified, false),
            "voice alone never unlocks privileged"
        );
        assert!(authorize(Privileged, Verified, true));
        assert!(!authorize(Privileged, Lost, true));
        for id in [Unverified, Verified, Lost] {
            for f in [false, true] {
                assert!(!authorize(Money, id, f), "money is not approved");
            }
        }
    }
}
