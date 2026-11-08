//! Ties the data sources together into a plain [`Snapshot`] the UI renders.
//! All I/O happens here; the UI never touches the filesystem.

use std::path::Path;
use std::time::Duration;

use crate::cpu::{self, CpuTracker};
use crate::format;
use crate::mem::{self, MemInfo};
use crate::sysfs::Fs;
use crate::temp::{CoreTempSensors, Reading};
use crate::topology::{CoreKind, Topology};

/// Where a logical CPU sits among its Hyper-Threading siblings, so the UI
/// can draw them as a visual pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadPos {
    /// The only thread on its physical core.
    Single,
    First,
    Middle,
    Last,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreRow {
    pub cpu: usize,
    pub kind: CoreKind,
    pub package: Option<u32>,
    pub core_id: Option<u32>,
    /// `None` while measuring.
    pub usage: Option<f32>,
    /// `None` without coretemp or when this core has no sensor.
    pub temp: Option<Reading>,
    pub thread: ThreadPos,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub cpu_model: Option<String>,
    pub uptime: Option<Duration>,
    /// `None` while measuring or if `/proc/stat` is unreadable.
    pub overall: Option<f32>,
    /// Package temperatures by package id.
    pub packages: Vec<(u32, Reading)>,
    pub has_coretemp: bool,
    pub hybrid: bool,
    /// Ordered for display: by core kind, package, physical core, CPU.
    pub cores: Vec<CoreRow>,
    pub mem: Option<MemInfo>,
}

impl Snapshot {
    /// True until the second `/proc/stat` sample arrives. False when
    /// `/proc/stat` is unreadable (no cores at all): that is "unavailable".
    pub fn measuring(&self) -> bool {
        self.overall.is_none()
            && !self.cores.is_empty()
            && self.cores.iter().all(|c| c.usage.is_none())
    }

    /// Number of distinct physical cores of `kind` (or of every kind).
    pub fn physical_cores(&self, kind: Option<CoreKind>) -> usize {
        let rows = self
            .cores
            .iter()
            .filter(|r| kind.is_none_or(|k| r.kind == k));
        rows.filter(|r| matches!(r.thread, ThreadPos::Single | ThreadPos::First))
            .count()
    }
}

pub struct Collector<F: Fs> {
    fs: F,
    cpu_model: Option<String>,
    sensors: CoreTempSensors,
    topology: Topology,
    /// CPUs the topology was read for; reread when the online set changes.
    topology_cpus: Vec<usize>,
    tracker: CpuTracker,
}

impl<F: Fs> Collector<F> {
    pub fn new(fs: F) -> Self {
        let cpu_model = fs
            .read(Path::new("/proc/cpuinfo"))
            .ok()
            .and_then(|s| cpu::parse_model_name(&s));
        let sensors = CoreTempSensors::discover(&fs);
        Collector {
            fs,
            cpu_model,
            sensors,
            topology: Topology::default(),
            topology_cpus: Vec::new(),
            tracker: CpuTracker::default(),
        }
    }

    pub fn collect(&mut self) -> Snapshot {
        let usage = match cpu::read_stat(&self.fs) {
            Ok(sample) => self.tracker.update(sample),
            Err(_) => {
                self.tracker.reset();
                Default::default()
            }
        };

        let online: Vec<usize> = usage.per_cpu.keys().copied().collect();
        if online != self.topology_cpus {
            self.topology = Topology::read(&self.fs, &online);
            self.topology_cpus = online;
        }

        let temps = self.sensors.read(&self.fs);
        let mut cores: Vec<CoreRow> = usage
            .per_cpu
            .iter()
            .map(|(&cpu, &usage)| {
                let topo = self.topology.get(cpu).copied();
                CoreRow {
                    cpu,
                    kind: topo.map_or(CoreKind::Uniform, |t| t.kind),
                    package: topo.and_then(|t| t.package_id),
                    core_id: topo.and_then(|t| t.core_id),
                    usage,
                    temp: topo.and_then(|t| temps.for_cpu(&t)),
                    thread: ThreadPos::Single,
                }
            })
            .collect();
        order_and_pair(&mut cores);

        Snapshot {
            cpu_model: self.cpu_model.clone(),
            uptime: self
                .fs
                .read(Path::new("/proc/uptime"))
                .ok()
                .and_then(|s| format::parse_uptime(&s)),
            overall: usage.overall,
            packages: temps.packages.into_iter().collect(),
            has_coretemp: !self.sensors.is_empty(),
            hybrid: self.topology.hybrid,
            cores,
            mem: mem::read_meminfo(&self.fs).ok(),
        }
    }
}

/// Sorts rows so HT siblings are adjacent and marks their position.
fn order_and_pair(rows: &mut [CoreRow]) {
    rows.sort_by_key(|r| {
        (
            r.kind,
            r.package.unwrap_or(u32::MAX),
            // CPUs with no known core sort by their own number.
            r.core_id.map_or((1, r.cpu), |c| (0, c as usize)),
            r.cpu,
        )
    });
    let sibling = |a: &CoreRow, b: &CoreRow| {
        a.core_id.is_some() && (a.kind, a.package, a.core_id) == (b.kind, b.package, b.core_id)
    };
    for i in 0..rows.len() {
        let prev = i > 0 && sibling(&rows[i - 1], &rows[i]);
        let next = i + 1 < rows.len() && sibling(&rows[i], &rows[i + 1]);
        rows[i].thread = match (prev, next) {
            (false, false) => ThreadPos::Single,
            (false, true) => ThreadPos::First,
            (true, true) => ThreadPos::Middle,
            (true, false) => ThreadPos::Last,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    fn cpus(s: &Snapshot) -> Vec<usize> {
        s.cores.iter().map(|r| r.cpu).collect()
    }

    #[test]
    fn quad_core_ht_end_to_end() {
        let fs = fixtures::quad_core_ht();
        let mut c = Collector::new(&fs);
        let first = c.collect();
        assert!(first.measuring());
        assert_eq!(first.cores.len(), 8);

        fs.set("/proc/stat", &fixtures::stat_busy(8, 0.25));
        let s = c.collect();
        assert!(!s.measuring());
        assert_eq!(s.cpu_model.as_deref(), Some("Intel Core i7-7700 @ 3.60GHz"));
        assert_eq!(cpus(&s), vec![0, 4, 1, 5, 2, 6, 3, 7]);
        assert_eq!(s.cores[0].thread, ThreadPos::First);
        assert_eq!(s.cores[1].thread, ThreadPos::Last);
        assert_eq!(s.cores[0].temp, s.cores[1].temp);
        assert!(
            s.cores
                .iter()
                .all(|r| (r.usage.unwrap() - 0.25).abs() < 1e-6)
        );
        assert!((s.overall.unwrap() - 0.25).abs() < 1e-6);
        assert_eq!(s.physical_cores(None), 4);
        assert_eq!(s.packages.len(), 1);
        assert!(s.has_coretemp && !s.hybrid);
        assert_eq!(s.uptime, Some(Duration::from_secs(3 * 86400 + 3600)));
        assert!(s.mem.unwrap().swap().is_some());
    }

    #[test]
    fn hybrid_groups_p_before_e() {
        let fs = fixtures::hybrid();
        let s = Collector::new(&fs).collect();
        assert!(s.hybrid);
        assert_eq!(s.physical_cores(Some(CoreKind::Performance)), 6);
        assert_eq!(s.physical_cores(Some(CoreKind::Efficiency)), 8);
        let kinds: Vec<CoreKind> = s.cores.iter().map(|r| r.kind).collect();
        assert!(kinds.is_sorted());
        assert_eq!(s.cores[12].kind, CoreKind::Efficiency);
        assert_eq!(s.cores[12].thread, ThreadPos::Single);
        assert!(s.cores.iter().all(|r| r.temp.is_some()));
    }

    #[test]
    fn vm_without_coretemp_still_reports_everything_else() {
        let fs = fixtures::vm();
        let mut c = Collector::new(&fs);
        c.collect();
        fs.set("/proc/stat", &fixtures::stat_busy(4, 0.5));
        let s = c.collect();
        assert!(!s.has_coretemp);
        assert!(s.packages.is_empty());
        assert!(
            s.cores
                .iter()
                .all(|r| r.temp.is_none() && r.usage.is_some())
        );
        assert_eq!(s.cores.len(), 4);
        assert!(s.mem.is_some());
        // cpu3 has no topology: it is listed, unpaired, after known cores.
        assert_eq!(s.cores.last().unwrap().cpu, 3);
        assert_eq!(s.cores.last().unwrap().thread, ThreadPos::Single);
    }

    #[test]
    fn no_swap_system() {
        let fs = fixtures::no_swap();
        let s = Collector::new(&fs).collect();
        assert_eq!(s.mem.unwrap().swap(), None);
    }

    #[test]
    fn unreadable_files_degrade_fields() {
        let fs = fixtures::quad_core_ht();
        let mut c = Collector::new(&fs);
        fs.remove_prefix("/proc");
        let s = c.collect();
        assert!(s.cores.is_empty());
        assert!(!s.measuring());
        assert_eq!(s.overall, None);
        assert_eq!(s.mem, None);
        assert_eq!(s.uptime, None);
        // Temperatures still come from /sys.
        assert_eq!(s.packages.len(), 1);
    }

    #[test]
    fn hotplug_rereads_topology() {
        let fs = fixtures::quad_core_ht();
        let mut c = Collector::new(&fs);
        c.collect();
        // cpu5 goes offline: its stat line disappears.
        let stat = fixtures::stat_busy(8, 0.1)
            .lines()
            .filter(|l| !l.starts_with("cpu5 "))
            .collect::<Vec<_>>()
            .join("\n");
        fs.set("/proc/stat", &stat);
        let s = c.collect();
        assert_eq!(cpus(&s), vec![0, 4, 1, 2, 6, 3, 7]);
        // cpu1 lost its sibling and is now shown alone.
        assert_eq!(s.cores[2].thread, ThreadPos::Single);
        assert!(s.cores.iter().all(|r| r.usage.is_some()));
    }
}
