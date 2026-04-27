use std::time::Instant;

pub fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

pub fn count_u32(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}
