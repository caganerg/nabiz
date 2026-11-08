//! Plain-text rendering of a snapshot, for `--once`. Pure: returns a string.

use std::fmt::Write;

use crate::collect::Snapshot;
use crate::format;

pub fn render(s: &Snapshot) -> String {
    let mut out = String::new();
    // Writing to a String cannot fail.
    let _ = write_snapshot(&mut out, s);
    out
}

fn write_snapshot(out: &mut String, s: &Snapshot) -> std::fmt::Result {
    let na = || "unavailable".to_owned();
    writeln!(out, "CPU      {}", s.cpu_model.clone().unwrap_or_else(na))?;
    writeln!(
        out,
        "Uptime   {}",
        s.uptime.map(format::uptime).unwrap_or_else(na)
    )?;
    let overall = match s.overall {
        Some(u) => format::percent(u),
        None if s.measuring() => "measuring…".into(),
        None => na(),
    };
    writeln!(out, "Overall  {overall}")?;
    if s.packages.is_empty() {
        writeln!(out, "Package  —")?;
    }
    for (id, r) in &s.packages {
        writeln!(
            out,
            "Package  {id}: {} (high {:.0}°C, crit {:.0}°C)",
            format::celsius(Some(r.celsius)),
            r.limits.high,
            r.limits.crit
        )?;
    }
    writeln!(
        out,
        "Cores    {} logical, {} physical{}{}",
        s.cores.len(),
        s.physical_cores(None),
        if s.hybrid { ", hybrid P/E" } else { "" },
        if s.has_coretemp { "" } else { ", no coretemp" },
    )?;
    for r in &s.cores {
        let id = |v: Option<u32>| v.map_or("?".to_owned(), |v| v.to_string());
        writeln!(
            out,
            "  cpu{:<3} {:1} core {:>3} pkg {:>2}  {:>5}  {:>5}",
            r.cpu,
            r.kind.short(),
            id(r.core_id),
            id(r.package),
            r.usage.map(format::percent).unwrap_or_else(|| "…".into()),
            format::celsius(r.temp.map(|t| t.celsius)),
        )?;
    }
    match s.mem {
        Some(m) => {
            writeln!(
                out,
                "RAM      {} / {} ({}), cache {}",
                format::bytes(m.used()),
                format::bytes(m.total),
                format::percent(ratio(m.used(), m.total)),
                format::bytes(m.cache_shown()),
            )?;
            match m.swap() {
                Some((used, total)) => writeln!(
                    out,
                    "Swap     {} / {} ({})",
                    format::bytes(used),
                    format::bytes(total),
                    format::percent(ratio(used, total)),
                )?,
                None => writeln!(out, "Swap     no swap")?,
            }
        }
        None => writeln!(out, "RAM      unavailable\nSwap     unavailable")?,
    }
    Ok(())
}

pub fn ratio(part: u64, whole: u64) -> f32 {
    if whole == 0 {
        0.0
    } else {
        (part as f64 / whole as f64) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collect::Collector;
    use crate::fixtures;

    #[test]
    fn renders_vm_without_temperatures() {
        let fs = fixtures::vm();
        let text = render(&Collector::new(&fs).collect());
        assert!(text.contains("Package  —"), "{text}");
        assert!(text.contains("no coretemp"));
        assert!(text.contains("Overall  measuring…"));
        assert!(text.contains("cpu3"));
    }

    #[test]
    fn renders_no_swap() {
        let fs = fixtures::no_swap();
        let text = render(&Collector::new(&fs).collect());
        assert!(text.contains("Swap     no swap"), "{text}");
        assert!(text.contains("Package  0: 50°C (high 80°C, crit 100°C)"));
    }
}
