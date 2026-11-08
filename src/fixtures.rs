//! String fixtures describing whole machines, for tests only.

use crate::sysfs::MemFs;

pub fn cpuinfo(model: &str) -> String {
    format!(
        "processor\t: 0\nvendor_id\t: GenuineIntel\ncpu family\t: 6\n\
         model name\t: {model}\nflags\t\t: fpu vme\n\n\
         processor\t: 1\nmodel name\t: {model}\n"
    )
}

/// Values in kB, like the real file.
pub fn meminfo(total: u64, available: u64, swap_total: u64) -> String {
    format!(
        "MemTotal:       {total} kB\n\
         MemFree:        {} kB\n\
         MemAvailable:   {available} kB\n\
         Buffers:          400000 kB\n\
         Cached:          8000000 kB\n\
         SwapCached:            0 kB\n\
         Shmem:           1000000 kB\n\
         SReclaimable:     600000 kB\n\
         SwapTotal:      {swap_total} kB\n\
         SwapFree:       {} kB\n\
         HugePages_Total:       0\n\
         Hugepagesize:       2048 kB\n",
        available / 2,
        swap_total / 4 * 3,
    )
}

/// `/proc/stat` for `n` CPUs at a baseline.
pub fn stat_idle(n: usize) -> String {
    stat_with(n, 0, 0)
}

/// `/proc/stat` advanced from [`stat_idle`] by 1000 jiffies per CPU, of
/// which `busy` is the busy fraction.
pub fn stat_busy(n: usize, busy: f64) -> String {
    let b = (busy * 1000.0).round() as u64;
    stat_with(n, b, 1000 - b)
}

fn stat_with(n: usize, extra_busy: u64, extra_idle: u64) -> String {
    let line = |name: &str, k: u64| {
        // user nice system idle iowait irq softirq steal guest guest_nice
        format!(
            "{name} {} 10 500 {} 100 0 20 0 0 0\n",
            (1000 + extra_busy) * k,
            (10_000 + extra_idle) * k
        )
    };
    let mut s = line("cpu ", n as u64);
    for i in 0..n {
        s += &line(&format!("cpu{i}"), 1);
    }
    s + "intr 12345 0 0\nctxt 999\nbtime 1700000000\nprocesses 42\nprocs_running 1\n"
}

fn topology(fs: MemFs, cpu: usize, core: u32, pkg: u32) -> MemFs {
    let base = format!("/sys/devices/system/cpu/cpu{cpu}/topology");
    fs.with(&format!("{base}/core_id"), &format!("{core}\n"))
        .with(&format!("{base}/physical_package_id"), &format!("{pkg}\n"))
}

fn sensor(fs: MemFs, hwmon: &str, i: u32, label: &str, milli: u32, limits: bool) -> MemFs {
    let base = format!("/sys/class/hwmon/{hwmon}/temp{i}");
    let fs = fs
        .with(&format!("{base}_label"), &format!("{label}\n"))
        .with(&format!("{base}_input"), &format!("{milli}\n"));
    if limits {
        fs.with(&format!("{base}_max"), "80000\n")
            .with(&format!("{base}_crit"), "100000\n")
            .with(&format!("{base}_crit_alarm"), "0\n")
    } else {
        fs
    }
}

/// Intel i7-7700: 4 cores / 8 threads; cpuN and cpuN+4 are siblings.
/// coretemp is hwmon3, after unrelated hwmon devices.
pub fn quad_core_ht() -> MemFs {
    let mut fs = MemFs::new()
        .with(
            "/proc/cpuinfo",
            &cpuinfo("Intel(R) Core(TM) i7-7700 CPU @ 3.60GHz"),
        )
        .with("/proc/stat", &stat_idle(8))
        .with("/proc/meminfo", &meminfo(16_000_000, 10_000_000, 2_000_000))
        .with("/proc/uptime", "262800.00 1000000.00\n")
        .with("/sys/class/hwmon/hwmon0/name", "acpitz\n")
        .with("/sys/class/hwmon/hwmon0/temp1_input", "27800\n")
        .with("/sys/class/hwmon/hwmon1/name", "nvme\n")
        .with("/sys/class/hwmon/hwmon1/temp1_label", "Composite\n")
        .with("/sys/class/hwmon/hwmon1/temp1_input", "38850\n")
        .with("/sys/class/hwmon/hwmon3/name", "coretemp\n");
    for cpu in 0..8 {
        fs = topology(fs, cpu, (cpu % 4) as u32, 0);
    }
    fs = sensor(fs, "hwmon3", 1, "Package id 0", 45000, true);
    for core in 0..4 {
        fs = sensor(
            fs,
            "hwmon3",
            core + 2,
            &format!("Core {core}"),
            41000 + core * 1000,
            true,
        );
    }
    fs
}

/// Intel i7-12700H: 6 P-cores with HT (cpu0–11, core_id 0,4,…,20) and
/// 8 E-cores (cpu12–19, core_id 24–31). E-core sensors lack max/crit.
pub fn hybrid() -> MemFs {
    let mut fs = MemFs::new()
        .with(
            "/proc/cpuinfo",
            &cpuinfo("12th Gen Intel(R) Core(TM) i7-12700H"),
        )
        .with("/proc/stat", &stat_idle(20))
        .with("/proc/meminfo", &meminfo(32_000_000, 20_000_000, 8_000_000))
        .with("/proc/uptime", "100.00 900.00\n")
        .with("/sys/devices/cpu_core/cpus", "0-11\n")
        .with("/sys/devices/cpu_atom/cpus", "12-19\n")
        .with("/sys/class/hwmon/hwmon5/name", "coretemp\n");
    for cpu in 0..12 {
        fs = topology(fs, cpu, (cpu / 2 * 4) as u32, 0);
    }
    for cpu in 12..20 {
        fs = topology(fs, cpu, (cpu + 12) as u32, 0);
    }
    fs = sensor(fs, "hwmon5", 1, "Package id 0", 62000, true);
    let mut i = 2;
    for core in (0..=20).step_by(4) {
        fs = sensor(
            fs,
            "hwmon5",
            i,
            &format!("Core {core}"),
            (50 + core) * 1000,
            true,
        );
        i += 1;
    }
    for core in 24..32 {
        fs = sensor(
            fs,
            "hwmon5",
            i,
            &format!("Core {core}"),
            (10 + core) * 1000,
            false,
        );
        i += 1;
    }
    fs
}

/// A 4-vCPU KVM guest: no coretemp, no hwmon at all, and cpu3 is missing
/// its topology directory.
pub fn vm() -> MemFs {
    let mut fs = MemFs::new()
        .with("/proc/cpuinfo", &cpuinfo("QEMU Virtual CPU version 2.5+"))
        .with("/proc/stat", &stat_idle(4))
        .with("/proc/meminfo", &meminfo(4_000_000, 3_000_000, 1_000_000))
        .with("/proc/uptime", "5000.00 19000.00\n");
    for cpu in 0..3 {
        fs = topology(fs, cpu, cpu as u32, 0);
    }
    fs
}

/// A dual-core laptop without any swap configured.
pub fn no_swap() -> MemFs {
    let mut fs = MemFs::new()
        .with(
            "/proc/cpuinfo",
            &cpuinfo("Intel(R) Core(TM) i3-7100U CPU @ 2.40GHz"),
        )
        .with("/proc/stat", &stat_idle(2))
        .with("/proc/meminfo", &meminfo(8_000_000, 4_000_000, 0))
        .with("/proc/uptime", "60.00 100.00\n")
        .with("/sys/class/hwmon/hwmon2/name", "coretemp\n");
    for cpu in 0..2 {
        fs = topology(fs, cpu, cpu as u32, 0);
    }
    fs = sensor(fs, "hwmon2", 1, "Package id 0", 50000, true);
    sensor(fs, "hwmon2", 2, "Core 0", 49000, true)
}

/// Two sockets, two cores each; both packages have a `Core 0` and `Core 1`.
pub fn dual_socket() -> MemFs {
    let mut fs = MemFs::new()
        .with("/proc/stat", &stat_idle(4))
        .with("/sys/class/hwmon/hwmon1/name", "coretemp\n")
        .with("/sys/class/hwmon/hwmon2/name", "coretemp\n");
    for cpu in 0..4 {
        fs = topology(fs, cpu, (cpu % 2) as u32, (cpu / 2) as u32);
    }
    fs = sensor(fs, "hwmon1", 1, "Package id 0", 45000, true);
    fs = sensor(fs, "hwmon1", 2, "Core 0", 40000, true);
    fs = sensor(fs, "hwmon1", 3, "Core 1", 41000, true);
    fs = sensor(fs, "hwmon2", 1, "Package id 1", 65000, true);
    fs = sensor(fs, "hwmon2", 2, "Core 0", 60000, true);
    sensor(fs, "hwmon2", 3, "Core 1", 61000, true)
}
