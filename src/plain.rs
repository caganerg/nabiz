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
    let na = || "okunamadı".to_owned();
    writeln!(out, "İşlemci    {}", s.cpu_model.clone().unwrap_or_else(na))?;
    writeln!(
        out,
        "Açık kalma {}",
        s.uptime.map(format::uptime).unwrap_or_else(na)
    )?;
    let overall = match s.overall {
        Some(u) => format::percent(u),
        None if s.measuring() => "ölçülüyor…".into(),
        None => na(),
    };
    writeln!(out, "Toplam     {overall}")?;
    if s.packages.is_empty() {
        writeln!(out, "Paket      —")?;
    }
    for (id, r) in &s.packages {
        writeln!(
            out,
            "Paket      {id}: {} (yüksek {:.0}°C, kritik {:.0}°C)",
            format::celsius(Some(r.celsius)),
            r.limits.high,
            r.limits.crit
        )?;
    }
    writeln!(
        out,
        "Çekirdek   {} mantıksal, {} fiziksel{}{}",
        s.cores.len(),
        s.physical_cores(None),
        if s.hybrid { ", hibrit P/E" } else { "" },
        if s.has_coretemp { "" } else { ", coretemp yok" },
    )?;
    for r in &s.cores {
        let id = |v: Option<u32>| v.map_or("?".to_owned(), |v| v.to_string());
        writeln!(
            out,
            "  cpu{:<3} {:1} çekirdek {:>3} paket {:>2}  {:>5}  {:>5}",
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
                "RAM        {} / {} ({}), önbellek {}",
                format::bytes(m.used()),
                format::bytes(m.total),
                format::percent(ratio(m.used(), m.total)),
                format::bytes(m.cache_shown()),
            )?;
            match m.swap() {
                Some((used, total)) => writeln!(
                    out,
                    "Swap       {} / {} ({})",
                    format::bytes(used),
                    format::bytes(total),
                    format::percent(ratio(used, total)),
                )?,
                None => writeln!(out, "Swap       swap yok")?,
            }
        }
        None => writeln!(out, "RAM        okunamadı\nSwap       okunamadı")?,
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
        assert!(text.contains("Paket      —"), "{text}");
        assert!(text.contains("coretemp yok"));
        assert!(text.contains("Toplam     ölçülüyor…"));
        assert!(text.contains("cpu3"));
    }

    #[test]
    fn renders_no_swap() {
        let fs = fixtures::no_swap();
        let text = render(&Collector::new(&fs).collect());
        assert!(text.contains("Swap       swap yok"), "{text}");
        assert!(text.contains("Paket      0: 50°C (yüksek 80°C, kritik 100°C)"));
    }
}
