//! Small pure formatting helpers shared by the TUI and `--once` output.

use std::time::Duration;

/// Bytes in binary units with one decimal (Turkish comma), never smaller
/// than MiB: `5,2 GiB`, `512,0 MiB`, `0,0 MiB`.
pub fn bytes(n: u64) -> String {
    const MIB: f64 = (1u64 << 20) as f64;
    const UNITS: [&str; 3] = ["MiB", "GiB", "TiB"];
    let mut value = n as f64 / MIB;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit]).replace('.', ",")
}

/// `3g 4sa`, `4sa 12dk`, `12dk`, `<1dk`.
pub fn uptime(d: Duration) -> String {
    let mins = d.as_secs() / 60;
    let (days, hours, mins) = (mins / 1440, (mins / 60) % 24, mins % 60);
    match (days, hours, mins) {
        (0, 0, 0) => "<1dk".to_owned(),
        (0, 0, m) => format!("{m}dk"),
        (0, h, m) => format!("{h}sa {m}dk"),
        (d, h, _) => format!("{d}g {h}sa"),
    }
}

/// `250 ms`, `1 sn`, `1,5 sn`.
pub fn interval(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms.is_multiple_of(1000) {
        format!("{} sn", ms / 1000)
    } else {
        format!("{:.1} sn", ms as f64 / 1000.0).replace('.', ",")
    }
}

/// `/proc/uptime`: first field is seconds since boot.
pub fn parse_uptime(s: &str) -> Option<Duration> {
    let secs: f64 = s.split_whitespace().next()?.parse().ok()?;
    (secs.is_finite() && secs >= 0.0).then(|| Duration::from_secs_f64(secs))
}

/// Usage fraction as a whole percentage, Turkish style: `%42`.
pub fn percent(frac: f32) -> String {
    format!("%{:.0}", (frac * 100.0).clamp(0.0, 100.0))
}

/// `48°C`, or `—` when unavailable.
pub fn celsius(c: Option<f32>) -> String {
    match c {
        Some(c) => format!("{c:.0}°C"),
        None => "—".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes() {
        assert_eq!(bytes(0), "0,0 MiB");
        assert_eq!(bytes(300 * 1024), "0,3 MiB");
        assert_eq!(bytes(512 * 1024 * 1024), "512,0 MiB");
        assert_eq!(bytes(16_050_768 * 1024), "15,3 GiB");
        assert_eq!(bytes(3 << 40), "3,0 TiB");
    }

    #[test]
    fn uptimes() {
        assert_eq!(uptime(Duration::from_secs(30)), "<1dk");
        assert_eq!(uptime(Duration::from_secs(12 * 60)), "12dk");
        assert_eq!(uptime(Duration::from_secs(4 * 3600 + 12 * 60)), "4sa 12dk");
        assert_eq!(
            uptime(Duration::from_secs(3 * 86400 + 4 * 3600 + 5)),
            "3g 4sa"
        );
    }

    #[test]
    fn uptime_file() {
        assert_eq!(
            parse_uptime("19655.33 110213.68\n"),
            Some(Duration::from_secs_f64(19655.33))
        );
        assert_eq!(parse_uptime(""), None);
        assert_eq!(parse_uptime("-1 0"), None);
        assert_eq!(parse_uptime("nan 0"), None);
    }

    #[test]
    fn intervals_and_values() {
        assert_eq!(interval(Duration::from_millis(250)), "250 ms");
        assert_eq!(interval(Duration::from_millis(1000)), "1 sn");
        assert_eq!(interval(Duration::from_millis(1500)), "1,5 sn");
        assert_eq!(percent(0.424), "%42");
        assert_eq!(percent(1.2), "%100");
        assert_eq!(celsius(Some(47.6)), "48°C");
        assert_eq!(celsius(None), "—");
    }
}
