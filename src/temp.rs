//! Intel `coretemp` discovery and reading.
//!
//! hwmon directories are located by their `name` file, never by number
//! (numbers change between boots). Discovery runs once; each tick only the
//! `temp*_input` files are re-read.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::sysfs::{Fs, read_parse};
use crate::topology::CpuTopo;

const HWMON_ROOT: &str = "/sys/class/hwmon";
/// TjMax fallback when `temp*_crit` is missing.
pub const DEFAULT_CRIT: f32 = 100.0;
/// How far below TjMax the "high" mark sits when `temp*_max` is missing.
const DEFAULT_HIGH_MARGIN: f32 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Label {
    /// `Package id X` (or `Physical id X` on old kernels).
    Package(u32),
    /// `Core N`, where N is the physical `core_id`.
    Core(u32),
    Other,
}

pub fn parse_label(s: &str) -> Label {
    let s = s.trim();
    let number = |rest: &str| rest.trim().parse::<u32>().ok();
    if let Some(n) = s
        .strip_prefix("Package id ")
        .or_else(|| s.strip_prefix("Physical id "))
        .and_then(number)
    {
        Label::Package(n)
    } else if let Some(n) = s.strip_prefix("Core ").and_then(number) {
        Label::Core(n)
    } else {
        Label::Other
    }
}

/// hwmon values are integer millidegrees Celsius.
pub fn parse_millicelsius(s: &str) -> Option<f32> {
    s.trim().parse::<i64>().ok().map(|m| m as f32 / 1000.0)
}

/// The chip's own thresholds, used to colour temperatures.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// `temp*_max`: where "hot" begins.
    pub high: f32,
    /// `temp*_crit`: TjMax.
    pub crit: f32,
}

impl Limits {
    pub fn new(max: Option<f32>, crit: Option<f32>) -> Self {
        let crit = crit
            .filter(|c| (20.0..=150.0).contains(c))
            .unwrap_or(DEFAULT_CRIT);
        let high = max
            .filter(|h| *h > 0.0 && *h <= crit)
            .unwrap_or(crit - DEFAULT_HIGH_MARGIN);
        Limits { high, crit }
    }

    /// Where `celsius` sits on the cool → hot scale, in `0.0..=1.0`.
    /// Everything below 40 % of TjMax is "cool"; `high` and above is "hot".
    pub fn heat(&self, celsius: f32) -> f32 {
        let cool = self.crit * 0.4;
        let span = (self.high - cool).max(1.0);
        ((celsius - cool) / span).clamp(0.0, 1.0)
    }
}

impl Default for Limits {
    fn default() -> Self {
        Limits::new(None, None)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    pub celsius: f32,
    pub limits: Limits,
}

impl Reading {
    pub fn heat(&self) -> f32 {
        self.limits.heat(self.celsius)
    }
    pub fn critical(&self) -> bool {
        self.celsius >= self.limits.crit
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Package(u32),
    Core { package: u32, core: u32 },
}

#[derive(Debug, Clone, PartialEq)]
struct Sensor {
    key: Key,
    input: PathBuf,
    limits: Limits,
}

/// Temperatures read in one tick.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CoreTemps {
    pub packages: BTreeMap<u32, Reading>,
    /// Keyed by `(package, core_id)`.
    pub cores: BTreeMap<(u32, u32), Reading>,
}

impl CoreTemps {
    /// Temperature of the physical core a logical CPU runs on. Hyper-Threading
    /// siblings share a `core_id` and so get the same reading.
    pub fn for_cpu(&self, topo: &CpuTopo) -> Option<Reading> {
        let core = topo.core_id?;
        let package = match topo.package_id {
            Some(p) => p,
            // Unknown package: only safe to guess on single-socket systems.
            None if self.packages.len() <= 1 => self.packages.keys().next().copied().unwrap_or(0),
            None => return None,
        };
        self.cores.get(&(package, core)).copied()
    }
}

/// The discovered set of coretemp sensors.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CoreTempSensors {
    sensors: Vec<Sensor>,
}

impl CoreTempSensors {
    pub fn discover(fs: &impl Fs) -> Self {
        let root = Path::new(HWMON_ROOT);
        let Ok(mut entries) = fs.list(root) else {
            return Self::default();
        };
        // Numeric order only matters as a tie-breaker for the fallback below.
        entries.retain(|e| e.starts_with("hwmon"));
        entries.sort_by_key(|e| e["hwmon".len()..].parse::<u32>().unwrap_or(u32::MAX));

        let mut sensors = Vec::new();
        let mut instance = 0u32;
        for entry in entries {
            let dir = root.join(&entry);
            // Very old kernels keep the attributes under `device/`.
            let Some(base) = [dir.clone(), dir.join("device")]
                .into_iter()
                .find(|b| is_coretemp(fs, b))
            else {
                continue;
            };
            sensors.extend(read_instance(fs, &base, instance));
            instance += 1;
        }
        CoreTempSensors { sensors }
    }

    pub fn is_empty(&self) -> bool {
        self.sensors.is_empty()
    }

    pub fn read(&self, fs: &impl Fs) -> CoreTemps {
        let mut temps = CoreTemps::default();
        for s in &self.sensors {
            let Some(celsius) = fs.read(&s.input).ok().and_then(|v| parse_millicelsius(&v)) else {
                continue;
            };
            let reading = Reading {
                celsius,
                limits: s.limits,
            };
            match s.key {
                Key::Package(p) => temps.packages.insert(p, reading),
                Key::Core { package, core } => temps.cores.insert((package, core), reading),
            };
        }
        temps
    }
}

fn is_coretemp(fs: &impl Fs, base: &Path) -> bool {
    fs.read(&base.join("name"))
        .is_ok_and(|n| n.contains("coretemp"))
}

/// Reads all labelled sensors of one coretemp instance (one per package).
fn read_instance(fs: &impl Fs, base: &Path, instance: u32) -> Vec<Sensor> {
    let Ok(files) = fs.list(base) else {
        return Vec::new();
    };
    let mut indices: Vec<u32> = files
        .iter()
        .filter_map(|f| f.strip_prefix("temp")?.strip_suffix("_label")?.parse().ok())
        .collect();
    indices.sort_unstable();

    let mut labelled = Vec::new();
    for i in indices {
        let label = fs
            .read(&base.join(format!("temp{i}_label")))
            .map(|l| parse_label(&l))
            .unwrap_or(Label::Other);
        if label == Label::Other {
            continue;
        }
        let milli = |attr: &str| {
            read_parse::<i64>(fs, base.join(format!("temp{i}_{attr}"))).map(|m| m as f32 / 1000.0)
        };
        let limits = Limits::new(milli("max"), milli("crit"));
        labelled.push((label, base.join(format!("temp{i}_input")), limits));
    }

    // Every core in this instance belongs to the package named by its
    // `Package id` label. Without one, fall back to discovery order.
    let package = labelled
        .iter()
        .find_map(|(l, _, _)| match l {
            Label::Package(p) => Some(*p),
            _ => None,
        })
        .unwrap_or(instance);

    labelled
        .into_iter()
        .filter_map(|(label, input, limits)| {
            let key = match label {
                Label::Package(p) => Key::Package(p),
                Label::Core(core) => Key::Core { package, core },
                Label::Other => return None,
            };
            Some(Sensor { key, input, limits })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;
    use crate::topology::{CoreKind, Topology};

    fn topo(core: Option<u32>, pkg: Option<u32>) -> CpuTopo {
        CpuTopo {
            core_id: core,
            package_id: pkg,
            kind: CoreKind::Uniform,
        }
    }

    #[test]
    fn labels() {
        assert_eq!(parse_label("Package id 0\n"), Label::Package(0));
        assert_eq!(parse_label("Package id 1"), Label::Package(1));
        assert_eq!(parse_label("Physical id 2"), Label::Package(2));
        assert_eq!(parse_label("Core 12\n"), Label::Core(12));
        assert_eq!(parse_label("Core"), Label::Other);
        assert_eq!(parse_label("Core x"), Label::Other);
        assert_eq!(parse_label("Tctl"), Label::Other);
        assert_eq!(parse_label(""), Label::Other);
    }

    #[test]
    fn millidegrees() {
        assert_eq!(parse_millicelsius("42000\n"), Some(42.0));
        assert_eq!(parse_millicelsius("41500"), Some(41.5));
        assert_eq!(parse_millicelsius("-5000"), Some(-5.0));
        assert_eq!(parse_millicelsius("hot"), None);
    }

    #[test]
    fn limits_fallbacks_and_heat() {
        let l = Limits::new(Some(94.0), Some(100.0));
        assert_eq!((l.high, l.crit), (94.0, 100.0));
        let d = Limits::new(None, None);
        assert_eq!((d.high, d.crit), (90.0, 100.0));
        // A max above crit is nonsense; fall back.
        let odd = Limits::new(Some(120.0), Some(105.0));
        assert_eq!((odd.high, odd.crit), (95.0, 105.0));
        assert_eq!(l.heat(30.0), 0.0);
        assert_eq!(l.heat(94.0), 1.0);
        assert_eq!(l.heat(120.0), 1.0);
        assert!(l.heat(67.0) > 0.4 && l.heat(67.0) < 0.6);
    }

    #[test]
    fn discovers_coretemp_by_name_not_number() {
        // coretemp is hwmon3, behind acpitz (hwmon0) and nvme (hwmon1).
        let fs = fixtures::quad_core_ht();
        let s = CoreTempSensors::discover(&fs);
        assert!(!s.is_empty());
        let t = s.read(&fs);
        assert_eq!(t.packages.len(), 1);
        assert_eq!(t.packages[&0].celsius, 45.0);
        assert_eq!(t.packages[&0].limits, Limits::new(Some(80.0), Some(100.0)));
        assert_eq!(t.cores.len(), 4);
        assert_eq!(t.cores[&(0, 3)].celsius, 44.0);
    }

    #[test]
    fn ht_siblings_share_temperature() {
        let fs = fixtures::quad_core_ht();
        let temps = CoreTempSensors::discover(&fs).read(&fs);
        let topo = Topology::read(&fs, &(0..8).collect::<Vec<_>>());
        for cpu in 0..4 {
            let a = temps.for_cpu(topo.get(cpu).unwrap());
            let b = temps.for_cpu(topo.get(cpu + 4).unwrap());
            assert!(a.is_some());
            assert_eq!(a, b);
        }
    }

    #[test]
    fn hybrid_core_ids_are_not_cpu_numbers() {
        let fs = fixtures::hybrid();
        let temps = CoreTempSensors::discover(&fs).read(&fs);
        let topo = Topology::read(&fs, &(0..20).collect::<Vec<_>>());
        // cpu2 is core_id 4, whose sensor reads 54 °C; there is no "Core 2".
        assert_eq!(temps.for_cpu(topo.get(2).unwrap()).unwrap().celsius, 54.0);
        assert!(!temps.cores.contains_key(&(0, 2)));
        // E-core cpu19 is core_id 31.
        assert_eq!(temps.for_cpu(topo.get(19).unwrap()).unwrap().celsius, 41.0);
        assert!((0..20).all(|c| temps.for_cpu(topo.get(c).unwrap()).is_some()));
    }

    #[test]
    fn dual_socket_maps_by_package() {
        let fs = fixtures::dual_socket();
        let temps = CoreTempSensors::discover(&fs).read(&fs);
        assert_eq!(temps.packages.len(), 2);
        // Both packages have a "Core 0"; package id must disambiguate.
        assert_eq!(
            temps.for_cpu(&topo(Some(0), Some(0))).unwrap().celsius,
            40.0
        );
        assert_eq!(
            temps.for_cpu(&topo(Some(0), Some(1))).unwrap().celsius,
            60.0
        );
        // Unknown package on a multi-socket machine: don't guess.
        assert_eq!(temps.for_cpu(&topo(Some(0), None)), None);
    }

    #[test]
    fn vm_without_coretemp_is_empty_not_error() {
        let fs = fixtures::vm();
        let s = CoreTempSensors::discover(&fs);
        assert!(s.is_empty());
        assert_eq!(s.read(&fs), CoreTemps::default());
        // No hwmon directory at all.
        assert!(CoreTempSensors::discover(&crate::sysfs::MemFs::new()).is_empty());
    }

    #[test]
    fn unreadable_input_skips_only_that_sensor() {
        let fs = fixtures::quad_core_ht();
        let s = CoreTempSensors::discover(&fs);
        fs.remove_prefix("/sys/class/hwmon/hwmon3/temp3_input");
        let t = s.read(&fs);
        assert_eq!(t.cores.len(), 3);
        assert!(!t.cores.contains_key(&(0, 1)));
    }

    #[test]
    fn missing_limits_use_fallback() {
        let fs = fixtures::hybrid();
        let t = CoreTempSensors::discover(&fs).read(&fs);
        // The hybrid fixture omits temp*_max/crit for E-cores.
        assert_eq!(t.cores[&(0, 31)].limits, Limits::default());
    }
}
