//! Bounded boot-scoped display mapping, never a scheduling or control clock.

use serde_json::Value;

/// One immutable OS wall-clock sample bracketed by the measurement clock.
/// Bracket uncertainty does not establish the absolute accuracy of the OS clock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TimeAnchor {
    pub(crate) boot_id: String,
    pub(crate) before_ns: u64,
    pub(crate) after_ns: u64,
    pub(crate) utc_us: i64,
}

impl TimeAnchor {
    /// An absent, unknown-quality or malformed extension cannot provide calendar time.
    #[allow(
        dead_code,
        reason = "read-only native children share the DTO without a Runtime client"
    )]
    pub(crate) fn from_hello(value: &Value, boot: &str) -> Option<Self> {
        let value = value.get("time_anchor")?;
        if boot.is_empty()
            || boot.len() > 128
            || value.get("boot_id")?.as_str()? != boot
            || value.get("source")?.as_str()? != "system_clock"
            || !value.get("utc_accuracy_ns")?.is_null()
        {
            return None;
        }
        let decimal = |key| {
            let text = value.get(key)?.as_str()?;
            (text.len() <= 20 && !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
                .then(|| text.parse::<u64>().ok())?
        };
        let before_ns = decimal("monotonic_before_ns")?;
        let after_ns = decimal("monotonic_after_ns")?;
        if after_ns.checked_sub(before_ns)? != decimal("bracket_uncertainty_ns")? {
            return None;
        }
        let text = value.get("utc_unix_us")?.as_str()?;
        if text.len() > 20
            || text.is_empty()
            || !text
                .strip_prefix('-')
                .unwrap_or(text)
                .bytes()
                .all(|b| b.is_ascii_digit())
        {
            return None;
        }
        Some(Self {
            boot_id: boot.to_owned(),
            before_ns,
            after_ns,
            utc_us: text.parse().ok()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn optional_binding_is_boot_checked_and_malformed_data_has_no_calendar_authority() {
        let value = json!({"time_anchor": {"boot_id":"a", "source":"system_clock",
            "utc_accuracy_ns":null, "monotonic_before_ns":"9", "monotonic_after_ns":"11",
            "bracket_uncertainty_ns":"2", "utc_unix_us":"-1000000"}});
        let anchor = TimeAnchor::from_hello(&value, "a").unwrap();
        assert_eq!(anchor.utc_us, -1_000_000);
        assert!(TimeAnchor::from_hello(&value, "new-boot").is_none());
        assert!(TimeAnchor::from_hello(&json!({}), "a").is_none());
        for (key, bad) in [
            ("monotonic_after_ns", json!("8")),
            ("bracket_uncertainty_ns", json!("1")),
            ("source", json!("arrival_time")),
            ("utc_unix_us", json!("+1")),
            ("utc_unix_us", json!("9223372036854775808")),
            ("utc_accuracy_ns", json!("0")),
        ] {
            let mut bad_value = value.clone();
            bad_value["time_anchor"][key] = bad;
            assert!(TimeAnchor::from_hello(&bad_value, "a").is_none(), "{key}");
        }
    }
}
