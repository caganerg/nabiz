//! RAM and swap from `/proc/meminfo`.

use std::collections::HashMap;
use std::path::Path;

use crate::error::{Error, Result};
use crate::sysfs::Fs;

/// All values in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemInfo {
    pub total: u64,
    pub available: u64,
    /// Reclaimable page cache + buffers (excluding shmem), for the lighter
    /// secondary bar segment.
    pub cache: u64,
    pub swap_total: u64,
    pub swap_free: u64,
}

impl MemInfo {
    /// `MemTotal − MemAvailable`.
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.available)
    }

    /// Cache shown next to "used", clamped so the two never exceed total.
    pub fn cache_shown(&self) -> u64 {
        self.cache.min(self.total.saturating_sub(self.used()))
    }

    /// `(used, total)` or `None` when the system has no swap.
    pub fn swap(&self) -> Option<(u64, u64)> {
        (self.swap_total > 0).then(|| {
            (
                self.swap_total.saturating_sub(self.swap_free),
                self.swap_total,
            )
        })
    }
}

pub fn parse_meminfo(s: &str) -> Result<MemInfo> {
    let mut fields: HashMap<&str, u64> = HashMap::new();
    for line in s.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let mut parts = rest.split_whitespace();
        let Some(value) = parts.next().and_then(|v| v.parse::<u64>().ok()) else {
            continue;
        };
        let bytes = match parts.next() {
            Some("kB") => value.saturating_mul(1024),
            _ => value,
        };
        fields.insert(key.trim(), bytes);
    }
    let get = |k: &str| fields.get(k).copied();

    let total = get("MemTotal").ok_or(Error::Parse("MemTotal missing"))?;
    let free = get("MemFree").unwrap_or(0);
    let buffers = get("Buffers").unwrap_or(0);
    let cached = get("Cached").unwrap_or(0);
    let reclaimable = get("SReclaimable").unwrap_or(0);
    let shmem = get("Shmem").unwrap_or(0);
    // MemAvailable appeared in Linux 3.14; approximate it on older kernels.
    let available = get("MemAvailable")
        .unwrap_or_else(|| free.saturating_add(buffers).saturating_add(cached))
        .min(total);
    let cache = buffers
        .saturating_add(cached)
        .saturating_add(reclaimable)
        .saturating_sub(shmem);
    let swap_total = get("SwapTotal").unwrap_or(0);
    let swap_free = get("SwapFree").unwrap_or(swap_total).min(swap_total);

    Ok(MemInfo {
        total,
        available,
        cache,
        swap_total,
        swap_free,
    })
}

pub fn read_meminfo(fs: &impl Fs) -> Result<MemInfo> {
    parse_meminfo(&fs.read(Path::new("/proc/meminfo"))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    const KIB: u64 = 1024;

    #[test]
    fn used_is_total_minus_available() {
        let m = parse_meminfo(&fixtures::meminfo(16_000_000, 10_000_000, 2_000_000)).unwrap();
        assert_eq!(m.total, 16_000_000 * KIB);
        assert_eq!(m.used(), 6_000_000 * KIB);
        assert_eq!(m.swap(), Some((500_000 * KIB, 2_000_000 * KIB)));
    }

    #[test]
    fn cache_segment() {
        let m = parse_meminfo(&fixtures::meminfo(16_000_000, 10_000_000, 0)).unwrap();
        // Buffers 400000 + Cached 8000000 + SReclaimable 600000 - Shmem 1000000
        assert_eq!(m.cache, 8_000_000 * KIB);
        assert!(m.used() + m.cache_shown() <= m.total);
    }

    #[test]
    fn no_swap() {
        let m = parse_meminfo(&fixtures::meminfo(8_000_000, 4_000_000, 0)).unwrap();
        assert_eq!(m.swap(), None);
        let m = parse_meminfo("MemTotal: 1000 kB\nMemAvailable: 500 kB\n").unwrap();
        assert_eq!(m.swap(), None);
    }

    #[test]
    fn missing_mem_available_falls_back() {
        let m =
            parse_meminfo("MemTotal: 1000 kB\nMemFree: 100 kB\nBuffers: 50 kB\nCached: 250 kB\n")
                .unwrap();
        assert_eq!(m.available, 400 * KIB);
    }

    #[test]
    fn malformed_input() {
        assert!(matches!(parse_meminfo(""), Err(Error::Parse(_))));
        assert!(parse_meminfo("MemTotal: lots kB\n").is_err());
        // Garbage lines are skipped, unit-less values kept as-is.
        let m = parse_meminfo("junk\nMemTotal: 2048 kB\nHugePages_Total: 3\n:\n").unwrap();
        assert_eq!(m.total, 2048 * KIB);
        assert_eq!(m.available, 0);
    }

    #[test]
    fn clamps_inconsistent_values() {
        let m = parse_meminfo(
            "MemTotal: 100 kB\nMemAvailable: 500 kB\nSwapTotal: 10 kB\nSwapFree: 20 kB\n",
        )
        .unwrap();
        assert_eq!(m.used(), 0);
        assert_eq!(m.swap(), Some((0, 10 * KIB)));
    }
}
