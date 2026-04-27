/// Parses an `--since` timestamp string.
///
/// Accepts ISO 8601 format (e.g., `2024-01-15T10:30:00Z`) or milliseconds since epoch.
///
/// # Errors
///
/// Returns [`crate::TalonError::InvalidSince`] if the timestamp cannot be parsed.
pub fn parse_since(timestamp: &str) -> Result<u64, crate::TalonError> {
    if let Some(ms) = parse_relative_duration(timestamp) {
        return Ok(now_ms().saturating_sub(ms));
    }

    // Try parsing as milliseconds since epoch (numeric string)
    if let Ok(ms) = timestamp.parse::<u64>() {
        return Ok(ms);
    }

    // Try parsing as ISO 8601 / RFC 3339:
    // - 2024-01-15T10:30:00Z
    // - 2024-01-15T10:30:00+00:00
    if let Ok(dt) =
        time::OffsetDateTime::parse(timestamp, &time::format_description::well_known::Rfc3339)
    {
        return Ok(unix_millis(dt));
    }

    // Try date-only format (YYYY-MM-DD); midnight UTC is implied.
    let date_format = time::macros::format_description!("[year]-[month]-[day]");
    if let Ok(date) = time::Date::parse(timestamp, date_format) {
        let dt = date
            .with_hms(0, 0, 0)
            .map(time::PrimitiveDateTime::assume_utc)
            .map_err(|err| crate::TalonError::InvalidSince {
                message: format!("00:00:00 is always valid (unreachable): {err}"),
            })?;
        return Ok(unix_millis(dt));
    }

    Err(crate::TalonError::InvalidSince {
        message: format!("unable to parse timestamp: {timestamp}"),
    })
}

fn parse_relative_duration(value: &str) -> Option<u64> {
    let trimmed = value.trim();
    if trimmed.len() < 2 {
        return None;
    }
    let (amount, unit) = trimmed.split_at(trimmed.len() - 1);
    let amount = amount.parse::<u64>().ok()?;
    let multiplier = match unit {
        "m" => 60 * 1000,
        "h" => 60 * 60 * 1000,
        "d" => 24 * 60 * 60 * 1000,
        "w" => 7 * 24 * 60 * 60 * 1000,
        _ => return None,
    };
    amount.checked_mul(multiplier)
}

/// Returns the current time in milliseconds since epoch.
#[must_use]
pub fn now_ms() -> u64 {
    unix_millis(time::OffsetDateTime::now_utc())
}

fn unix_millis(dt: time::OffsetDateTime) -> u64 {
    let nanos = dt.unix_timestamp_nanos();
    if nanos < 0 {
        return 0;
    }
    let millis = nanos / 1_000_000;
    u64::try_from(millis).unwrap_or(u64::MAX)
}

/// Default tombstone retention period: 90 days in milliseconds.
pub const TOMBSTONE_RETENTION_MS: u64 = 90 * 24 * 60 * 60 * 1000;
