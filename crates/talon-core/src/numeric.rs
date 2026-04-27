pub fn count_u32(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}
