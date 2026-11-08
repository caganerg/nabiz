//! CPU topology: which physical core and package each logical CPU belongs
//! to, and whether it is a Performance or Efficiency core on hybrid Intel
//! parts (Alder Lake and newer).

use std::collections::{BTreeMap, BTreeSet};

use crate::sysfs::{Fs, read_parse};

const CPU_CORE_LIST: &str = "/sys/devices/cpu_core/cpus";
const CPU_ATOM_LIST: &str = "/sys/devices/cpu_atom/cpus";

/// Parses a kernel CPU range list such as `0-3,8,10-11` into a sorted,
/// de-duplicated list. An empty string is an empty list; anything malformed
/// yields `None`.
pub fn parse_cpu_list(s: &str) -> Option<Vec<usize>> {
    /// Guard against absurd ranges in corrupted input.
    const MAX_SPAN: usize = 1 << 16;

    let mut out = Vec::new();
    let s = s.trim();
    if s.is_empty() {
        return Some(out);
    }
    for part in s.split(',') {
        let part = part.trim();
        match part.split_once('-') {
            Some((lo, hi)) => {
                let lo: usize = lo.trim().parse().ok()?;
                let hi: usize = hi.trim().parse().ok()?;
                if hi < lo || hi - lo > MAX_SPAN {
                    return None;
                }
                out.extend(lo..=hi);
            }
            None => out.push(part.parse().ok()?),
        }
    }
    out.sort_unstable();
    out.dedup();
    Some(out)
}

/// The kind of core a logical CPU runs on. The declaration order is the
/// display order: P-cores first, then E-cores, then everything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CoreKind {
    Performance,
    Efficiency,
    /// Non-hybrid CPU (or a CPU not listed in either hybrid set).
    Uniform,
}

impl CoreKind {
    pub fn short(self) -> &'static str {
        match self {
            CoreKind::Performance => "P",
            CoreKind::Efficiency => "E",
            CoreKind::Uniform => "",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuTopo {
    pub core_id: Option<u32>,
    pub package_id: Option<u32>,
    pub kind: CoreKind,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Topology {
    /// Keyed by logical CPU number.
    pub cpus: BTreeMap<usize, CpuTopo>,
    /// True when `cpu_core` / `cpu_atom` lists were found.
    pub hybrid: bool,
}

impl Topology {
    /// Pure constructor: `entries` are `(cpu, core_id, package_id)`; `p` and
    /// `e` are the parsed hybrid CPU lists, if present.
    pub fn build(
        entries: impl IntoIterator<Item = (usize, Option<u32>, Option<u32>)>,
        p: Option<&[usize]>,
        e: Option<&[usize]>,
    ) -> Self {
        let p: BTreeSet<usize> = p.unwrap_or_default().iter().copied().collect();
        let e: BTreeSet<usize> = e.unwrap_or_default().iter().copied().collect();
        let hybrid = !p.is_empty() || !e.is_empty();
        let cpus = entries
            .into_iter()
            .map(|(cpu, core_id, package_id)| {
                let kind = if p.contains(&cpu) {
                    CoreKind::Performance
                } else if e.contains(&cpu) {
                    CoreKind::Efficiency
                } else {
                    CoreKind::Uniform
                };
                (
                    cpu,
                    CpuTopo {
                        core_id,
                        package_id,
                        kind,
                    },
                )
            })
            .collect();
        Topology { cpus, hybrid }
    }

    /// Reads topology for the given logical CPUs. Missing files (offline
    /// CPUs, containers, odd kernels) simply leave the field as `None`.
    pub fn read(fs: &impl Fs, cpus: &[usize]) -> Self {
        let list = |path: &str| fs.read(path.as_ref()).ok().and_then(|s| parse_cpu_list(&s));
        let p = list(CPU_CORE_LIST);
        let e = list(CPU_ATOM_LIST);
        let entries = cpus.iter().map(|&cpu| {
            let base = format!("/sys/devices/system/cpu/cpu{cpu}/topology");
            (
                cpu,
                read_parse::<u32>(fs, format!("{base}/core_id")),
                // `physical_package_id` can be -1 ("unknown"), which fails
                // the u32 parse and correctly becomes `None`.
                read_parse::<u32>(fs, format!("{base}/physical_package_id")),
            )
        });
        Self::build(entries, p.as_deref(), e.as_deref())
    }

    pub fn get(&self, cpu: usize) -> Option<&CpuTopo> {
        self.cpus.get(&cpu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    #[test]
    fn cpu_list_ranges_and_singles() {
        assert_eq!(
            parse_cpu_list("0-3,8,10-11"),
            Some(vec![0, 1, 2, 3, 8, 10, 11])
        );
        assert_eq!(parse_cpu_list("0-11\n"), Some((0..=11).collect()));
        assert_eq!(parse_cpu_list("5"), Some(vec![5]));
        assert_eq!(parse_cpu_list(" 3 , 1-2 "), Some(vec![1, 2, 3]));
    }

    #[test]
    fn cpu_list_empty_and_duplicates() {
        assert_eq!(parse_cpu_list(""), Some(vec![]));
        assert_eq!(parse_cpu_list("\n"), Some(vec![]));
        assert_eq!(parse_cpu_list("1,1,0-1"), Some(vec![0, 1]));
    }

    #[test]
    fn cpu_list_malformed() {
        assert_eq!(parse_cpu_list("a-b"), None);
        assert_eq!(parse_cpu_list("3-1"), None);
        assert_eq!(parse_cpu_list("1,,2"), None);
        assert_eq!(parse_cpu_list("-1"), None);
        assert_eq!(parse_cpu_list("0-99999999"), None);
    }

    #[test]
    fn build_marks_hybrid_kinds() {
        let t = Topology::build(
            [
                (0, Some(0), Some(0)),
                (1, Some(8), Some(0)),
                (2, None, None),
            ],
            Some(&[0]),
            Some(&[1]),
        );
        assert!(t.hybrid);
        assert_eq!(t.get(0).unwrap().kind, CoreKind::Performance);
        assert_eq!(t.get(1).unwrap().kind, CoreKind::Efficiency);
        assert_eq!(t.get(2).unwrap().kind, CoreKind::Uniform);
    }

    #[test]
    fn quad_core_ht_siblings_share_core_ids() {
        let fs = fixtures::quad_core_ht();
        let t = Topology::read(&fs, &(0..8).collect::<Vec<_>>());
        assert!(!t.hybrid);
        for cpu in 0..4 {
            let a = t.get(cpu).unwrap();
            let b = t.get(cpu + 4).unwrap();
            assert_eq!(a.core_id, Some(cpu as u32));
            assert_eq!(
                a.core_id,
                b.core_id,
                "cpu{cpu} and cpu{} are siblings",
                cpu + 4
            );
            assert_eq!(a.kind, CoreKind::Uniform);
        }
    }

    #[test]
    fn hybrid_fixture_detects_p_and_e() {
        let fs = fixtures::hybrid();
        let t = Topology::read(&fs, &(0..20).collect::<Vec<_>>());
        assert!(t.hybrid);
        assert!((0..12).all(|c| t.get(c).unwrap().kind == CoreKind::Performance));
        assert!((12..20).all(|c| t.get(c).unwrap().kind == CoreKind::Efficiency));
        // P-core HT siblings: cpu0/cpu1 share core 0, cpu2/cpu3 share core 4, …
        assert_eq!(t.get(1).unwrap().core_id, Some(0));
        assert_eq!(t.get(3).unwrap().core_id, Some(4));
        assert_eq!(t.get(12).unwrap().core_id, Some(24));
    }

    #[test]
    fn missing_topology_files_degrade_to_none() {
        let fs = fixtures::vm();
        // cpu3 has no topology directory in the VM fixture.
        let t = Topology::read(&fs, &[0, 3]);
        assert_eq!(t.get(0).unwrap().core_id, Some(0));
        assert_eq!(t.get(3).unwrap().core_id, None);
        assert_eq!(t.get(3).unwrap().package_id, None);
    }

    #[test]
    fn negative_package_id_is_unknown() {
        let fs = crate::sysfs::MemFs::new()
            .with("/sys/devices/system/cpu/cpu0/topology/core_id", "0\n")
            .with(
                "/sys/devices/system/cpu/cpu0/topology/physical_package_id",
                "-1\n",
            );
        let t = Topology::read(&fs, &[0]);
        assert_eq!(t.get(0).unwrap().package_id, None);
    }
}
