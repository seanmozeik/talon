//! Quantization helpers for sqlite-vec int8 embeddings.

/// Converts an arbitrary f32 embedding into sqlite-vec's int8 vector format.
///
/// The output is a unit-vector quantization: each component is normalized by
/// the source vector norm, scaled to i8 range, rounded, and clamped to keep
/// `-128` unused.
#[must_use]
pub fn f32_to_i8_normalized(v: &[f32]) -> Vec<i8> {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        v.iter()
            .map(|x| rounded_unit_component_to_i8(x * 127.0 / norm))
            .collect()
    } else {
        vec![0_i8; v.len()]
    }
}

fn rounded_unit_component_to_i8(value: f32) -> i8 {
    let rounded = value.round().clamp(-127.0, 127.0);
    if rounded >= 0.0 {
        let mut candidate = 0_i8;
        while f32::from(candidate) < rounded {
            candidate += 1;
        }
        candidate
    } else {
        let mut candidate = 0_i8;
        while f32::from(candidate) > rounded {
            candidate -= 1;
        }
        candidate
    }
}

/// Computes the integer dot product of two i8 vectors.
#[must_use]
pub fn i8_dot(a: &[i8], b: &[i8]) -> i32 {
    a.iter()
        .zip(b)
        .map(|(&left, &right)| i32::from(left) * i32::from(right))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f32_to_i8_normalized_scales_unit_vector_to_i8_range() {
        assert_eq!(f32_to_i8_normalized(&[3.0, 4.0, 0.0]), [76, 102, 0]);
    }

    #[test]
    fn f32_to_i8_normalized_preserves_empty_vector() {
        assert!(f32_to_i8_normalized(&[]).is_empty());
    }

    #[test]
    fn f32_to_i8_normalized_zero_vector_stays_zeroed() {
        assert_eq!(f32_to_i8_normalized(&[0.0, 0.0, 0.0]), [0, 0, 0]);
    }

    #[test]
    fn i8_dot_multiplies_component_pairs() {
        assert_eq!(i8_dot(&[2, -3, 4], &[-5, 6, 7]), 0);
    }
}
