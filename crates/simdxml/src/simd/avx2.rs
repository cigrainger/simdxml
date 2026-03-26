//! AVX2 (x86_64) structural character classifier.
//!
//! Processes 64 bytes at a time using 256-bit AVX2 registers to produce
//! bitmasks for '<' and '>' positions, with quote masking to ignore
//! structural characters inside attribute values.
//!
//! TODO: Implement real AVX2 SIMD; currently delegates to scalar fallback.

use super::StructuralIndex;

/// Classify structural characters using AVX2 vector instructions.
/// Processes the entire input in one pass, producing bitmasks for Stage 2.
#[cfg(target_arch = "x86_64")]
pub fn classify_avx2(input: &[u8]) -> StructuralIndex {
    // TODO: implement real AVX2 SIMD
    super::scalar::classify_scalar(input)
}
