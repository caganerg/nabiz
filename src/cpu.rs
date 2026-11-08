//! Per-CPU usage from `/proc/stat`, plus the model name from `/proc/cpuinfo`.

use std::collections::BTreeMap;
use std::path::Path;

use crate::error::Result;
use crate::sysfs::Fs;

/// Aggregated jiffy counters for one CPU line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CpuTimes {
    /// user + nice + system + irq + softirq + steal
    pub busy: u64,
    /// idle + iowait
    pub idle: u64,
}

impl CpuTimes {
    /// Builds counters from the numeric fields of a `cpu` line. Fields that
    /// old kernels don't report are treated as zero; fewer than four fields
    /// (user, nice, system, idle) is malformed. `guest`/`guest_nice` are
    /// already included in `user`/`nice`, so they are ignored.
    fn from_fields(f: &[u64]) -> Option<Self> {
        if f.len() < 4 {
            return None;
        }
        let at = |i: usize| f.get(i).copied().unwrap_or(0);
        let sum = |idx: &[usize]| idx.iter().fold(0u64, |acc, &i| acc.saturating_add(at(i)));
        Some(CpuTimes {
            busy: sum(&[0, 1, 2, 5, 6, 7]),
            idle: sum(&[3, 4]),
        })
    }
}

/// Usage in `0.0..=1.0` between two samples, or `None` when no time has
/// elapsed. Saturating subtraction absorbs the occasional backwards step of
/// `iowait`, a known kernel quirk.
pub fn usage_between(prev: CpuTimes, cur: CpuTimes) -> Option<f32> {
    let busy = cur.busy.saturating_sub(prev.busy);
    let idle = cur.idle.saturating_sub(prev.idle);
    let total = busy.saturating_add(idle);
    if total == 0 {
        return None;
    }
    Some((busy as f64 / total as f64).clamp(0.0, 1.0) as f32)
}

/// One reading of `/proc/stat`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatSample {
    /// The aggregate `cpu` line.
    pub total: Option<CpuTimes>,
    /// `cpuN` lines keyed by N. Offline CPUs are simply absent.
    pub cpus: BTreeMap<usize, CpuTimes>,
}

pub fn parse_stat(s: &str) -> StatSample {
    let mut sample = StatSample::default();
    for line in s.lines() {
        let mut fields = line.split_ascii_whitespace();
        let Some(suffix) = fields.next().and_then(|name| name.strip_prefix("cpu")) else {
            continue;
        };
        let Some(numbers) = fields
            .map(|f| f.parse::<u64>().ok())
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let Some(times) = CpuTimes::from_fields(&numbers) else {
            continue;
        };
        if suffix.is_empty() {
            sample.total = Some(times);
        } else if let Ok(id) = suffix.parse::<usize>() {
            sample.cpus.insert(id, times);
        }
    }
    sample
}

pub fn read_stat(fs: &impl Fs) -> Result<StatSample> {
    Ok(parse_stat(&fs.read(Path::new("/proc/stat"))?))
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CpuUsage {
    /// `None` while measuring (first frame) or if unavailable.
    pub overall: Option<f32>,
    /// Every CPU present in the latest sample; `None` while measuring.
    pub per_cpu: BTreeMap<usize, Option<f32>>,
}

/// Keeps the previous sample so usage can be computed as a delta. CPUs are
/// matched by number, so a CPU going offline never shifts the others.
#[derive(Debug, Default)]
pub struct CpuTracker {
    prev: Option<StatSample>,
}

impl CpuTracker {
    pub fn update(&mut self, cur: StatSample) -> CpuUsage {
        let prev = self.prev.as_ref();
        let overall = match (prev.and_then(|p| p.total), cur.total) {
            (Some(a), Some(b)) => usage_between(a, b),
            _ => None,
        };
        let per_cpu = cur
            .cpus
            .iter()
            .map(|(&id, &now)| {
                let before = prev.and_then(|p| p.cpus.get(&id));
                (id, before.and_then(|&b| usage_between(b, now)))
            })
            .collect();
        self.prev = Some(cur);
        CpuUsage { overall, per_cpu }
    }

    /// Forget the previous sample (used when `/proc/stat` becomes unreadable).
    pub fn reset(&mut self) {
        self.prev = None;
    }
}

/// The first `model name` from `/proc/cpuinfo`, tidied up.
pub fn parse_model_name(cpuinfo: &str) -> Option<String> {
    cpuinfo
        .lines()
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim() == "model name")
        .map(|(_, v)| clean_model_name(v))
        .filter(|v| !v.is_empty())
}

/// `Intel(R) Core(TM) i5-8500T CPU @ 2.10GHz` → `Intel Core i5-8500T @ 2.10GHz`.
pub fn clean_model_name(raw: &str) -> String {
    let mut s = raw.to_owned();
    for noise in ["(R)", "(r)", "(TM)", "(tm)", "®", "™"] {
        s = s.replace(noise, "");
    }
    let words: Vec<&str> = s.split_whitespace().collect();
    // Drop Intel's redundant "CPU" before the clock ("… i5-8500T CPU @ 2.10GHz")
    // or at the very end, but keep it elsewhere ("QEMU Virtual CPU version …").
    let keep =
        |i: usize| words[i] != "CPU" || !matches!(words.get(i + 1).copied(), None | Some("@"));
    let kept: Vec<&str> = (0..words.len())
        .filter(|&i| keep(i))
        .map(|i| words[i])
        .collect();
    kept.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    const STAT: &str = "\
cpu  871790 2401 188997 11021362 8089 0 5229 0 0 0
cpu0 100 10 50 800 40 5 3 2 7 0
cpu1 153515 173 31318 1831528 1349 0 503 0 0 0
intr 1234 0 0
ctxt 5678
btime 1700000000
processes 4242
procs_running 2
softirq 1 2 3
";

    #[test]
    fn parses_busy_and_idle_formula() {
        let s = parse_stat(STAT);
        // busy = user+nice+system+irq+softirq+steal = 100+10+50+5+3+2 (guest ignored)
        // idle = idle+iowait = 800+40
        assert_eq!(
            s.cpus[&0],
            CpuTimes {
                busy: 170,
                idle: 840
            }
        );
        assert_eq!(s.cpus.len(), 2);
        assert_eq!(
            s.total,
            Some(CpuTimes {
                busy: 871790 + 2401 + 188997 + 5229,
                idle: 11021362 + 8089,
            })
        );
    }

    #[test]
    fn ignores_non_cpu_and_malformed_lines() {
        let s = parse_stat("cpu0 1 2\ncpuX 1 2 3 4\ncpu1 1 2 3 x\ncpu2 1 2 3 4\ncpufreq 9\n");
        assert_eq!(s.cpus.keys().copied().collect::<Vec<_>>(), vec![2]);
        assert_eq!(s.cpus[&2], CpuTimes { busy: 6, idle: 4 });
        assert_eq!(s.total, None);
    }

    #[test]
    fn short_lines_from_old_kernels() {
        // 2.4-era kernels only report four fields.
        let s = parse_stat("cpu0 10 0 10 80\n");
        assert_eq!(s.cpus[&0], CpuTimes { busy: 20, idle: 80 });
    }

    #[test]
    fn usage_delta() {
        let a = CpuTimes {
            busy: 100,
            idle: 900,
        };
        let b = CpuTimes {
            busy: 150,
            idle: 950,
        };
        assert_eq!(usage_between(a, b), Some(0.5));
        assert_eq!(usage_between(a, a), None);
        // iowait going backwards must not panic or produce nonsense.
        let c = CpuTimes {
            busy: 200,
            idle: 899,
        };
        assert_eq!(usage_between(a, c), Some(1.0));
    }

    #[test]
    fn first_sample_is_measuring() {
        let mut t = CpuTracker::default();
        let u = t.update(parse_stat(STAT));
        assert_eq!(u.overall, None);
        assert!(u.per_cpu.values().all(Option::is_none));
        assert_eq!(u.per_cpu.len(), 2);
    }

    #[test]
    fn offline_cpu_does_not_shift_others() {
        let mut t = CpuTracker::default();
        t.update(parse_stat(
            "cpu 0 0 0 0\ncpu0 0 0 0 100\ncpu1 0 0 0 100\ncpu2 0 0 0 100\n",
        ));
        // cpu1 goes offline; cpu2 is 100% busy, cpu0 idle.
        let u = t.update(parse_stat(
            "cpu 0 0 0 0\ncpu0 0 0 0 200\ncpu2 100 0 0 100\n",
        ));
        assert_eq!(u.per_cpu.get(&0), Some(&Some(0.0)));
        assert_eq!(u.per_cpu.get(&2), Some(&Some(1.0)));
        assert!(!u.per_cpu.contains_key(&1));
        // cpu1 comes back: it is measuring again, others unaffected.
        let u = t.update(parse_stat(
            "cpu 0 0 0 0\ncpu0 50 0 0 250\ncpu1 0 0 0 300\ncpu2 150 0 0 150\n",
        ));
        assert_eq!(u.per_cpu.get(&1), Some(&None));
        assert_eq!(u.per_cpu.get(&0), Some(&Some(0.5)));
    }

    #[test]
    fn model_name_cleanup() {
        assert_eq!(
            parse_model_name(&fixtures::cpuinfo(
                "Intel(R) Core(TM) i5-8500T CPU @ 2.10GHz"
            )),
            Some("Intel Core i5-8500T @ 2.10GHz".to_owned())
        );
        assert_eq!(
            clean_model_name("12th Gen Intel(R) Core(TM) i7-12700H"),
            "12th Gen Intel Core i7-12700H"
        );
        assert_eq!(
            clean_model_name("QEMU Virtual CPU version 2.5+"),
            "QEMU Virtual CPU version 2.5+"
        );
        assert_eq!(clean_model_name("Intel(R) Xeon(R) CPU"), "Intel Xeon");
        assert_eq!(parse_model_name("processor : 0\n"), None);
        assert_eq!(parse_model_name("model name\t: \n"), None);
    }
}
