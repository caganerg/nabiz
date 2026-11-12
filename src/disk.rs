//! Disk usage: the block-device filesystems in `/proc/mounts`, each sized
//! with `statvfs`.

use std::path::Path;

use crate::plain::ratio;
use crate::sysfs::Fs;

/// One mounted filesystem. Sizes in bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disk {
    /// Where it is mounted, e.g. `/` or `/home`.
    pub mount: String,
    /// The block device, e.g. `/dev/nvme0n1p2`.
    pub device: String,
    pub total: u64,
    pub used: u64,
    /// Free space an unprivileged user may still use.
    pub avail: u64,
}

impl Disk {
    /// The device's short name, as `lsblk` lists it: `/dev/nvme0n1p2` is
    /// `nvme0n1p2`, `/dev/mapper/vg-data` is `vg-data`.
    pub fn name(&self) -> &str {
        self.device.rsplit('/').next().unwrap_or(&self.device)
    }

    /// Fullness as `df` reports it: `used / (used + avail)`, so the space
    /// reserved for root counts as unavailable rather than free.
    pub fn frac(&self) -> f32 {
        ratio(self.used, self.used.saturating_add(self.avail))
    }
}

/// A mount worth showing, before it is sized.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Mount {
    device: String,
    point: String,
}

/// Filesystems that live on a block device but aren't a disk the user
/// fills: read-only images and the like.
const SKIP_FSTYPES: [&str; 3] = ["squashfs", "iso9660", "udf"];

/// Real disks from `/proc/mounts`: backed by a `/dev/` block device (no loop
/// devices), not under `/boot`, each device once (bind mounts and btrfs
/// subvolumes repeat it; the first mount wins).
fn parse_mounts(s: &str) -> Vec<Mount> {
    let mut out: Vec<Mount> = Vec::new();
    for line in s.lines() {
        let mut fields = line.split_whitespace();
        let (Some(device), Some(point), Some(fstype)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let (device, point) = (unescape(device), unescape(point));
        let boot = point == "/boot" || point.starts_with("/boot/");
        if !device.starts_with("/dev/")
            || device.starts_with("/dev/loop")
            || boot
            || SKIP_FSTYPES.contains(&fstype)
            || out.iter().any(|m| m.device == device)
        {
            continue;
        }
        out.push(Mount { device, point });
    }
    out
}

/// `/proc/mounts` writes space, tab, newline and backslash as `\ooo`.
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let octal = b
            .get(i + 1..i + 4)
            .filter(|d| b[i] == b'\\' && d.iter().all(|c| (b'0'..=b'7').contains(c)))
            .and_then(|d| u8::from_str_radix(std::str::from_utf8(d).ok()?, 8).ok());
        match octal {
            Some(c) => {
                out.push(c);
                i += 4;
            }
            None => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Every disk, `/` first and then the largest first. Mounts that can't be
/// sized (or report zero size) are left out. `None` only when
/// `/proc/mounts` itself is unreadable.
pub fn read_disks(fs: &impl Fs) -> Option<Vec<Disk>> {
    let mounts = fs.read(Path::new("/proc/mounts")).ok()?;
    let mut disks: Vec<Disk> = parse_mounts(&mounts)
        .into_iter()
        .filter_map(|m| {
            let space = fs.space(Path::new(&m.point)).ok()?;
            (space.total > 0).then(|| Disk {
                used: space.total.saturating_sub(space.free),
                avail: space.avail.min(space.free),
                total: space.total,
                mount: m.point,
                device: m.device,
            })
        })
        .collect();
    disks.sort_by_key(|d| (d.mount != "/", std::cmp::Reverse(d.total)));
    Some(disks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysfs::MemFs;

    const GIB: u64 = 1 << 30;

    const MOUNTS: &str = "\
sysfs /sys sysfs rw,nosuid 0 0
proc /proc proc rw 0 0
/dev/nvme0n1p2 / btrfs rw,subvol=/@ 0 0
tmpfs /run tmpfs rw 0 0
/dev/nvme0n1p1 /boot/efi vfat rw 0 0
/dev/loop0 /snap/core/1 squashfs ro 0 0
/dev/nvme0n1p2 /home btrfs rw,subvol=/@home 0 0
/dev/sda1 /media/cagan/Yedek\\040Disk ext4 rw 0 0
/dev/sr0 /media/cdrom iso9660 ro 0 0
/dev/mapper/vg-data /data ext4 rw 0 0
";

    #[test]
    fn keeps_only_real_disks_once() {
        let points: Vec<String> = parse_mounts(MOUNTS).into_iter().map(|m| m.point).collect();
        assert_eq!(points, ["/", "/media/cagan/Yedek Disk", "/data"]);
    }

    #[test]
    fn unescapes_octal() {
        assert_eq!(unescape("/a\\040b\\011c\\134d"), "/a b\tc\\d");
        // Not an escape: left as is.
        assert_eq!(unescape("/x\\9y\\04"), "/x\\9y\\04");
    }

    #[test]
    fn sizes_and_orders() {
        let fs = MemFs::new()
            .with("/proc/mounts", MOUNTS)
            .with_space("/", 200 * GIB, 50 * GIB)
            .with_space("/media/cagan/Yedek Disk", 1000 * GIB, 900 * GIB)
            .with_space("/data", 0, 0);
        let disks = read_disks(&fs).unwrap();
        let points: Vec<&str> = disks.iter().map(|d| d.mount.as_str()).collect();
        // /data has zero size and is dropped; / leads despite being smaller.
        assert_eq!(points, ["/", "/media/cagan/Yedek Disk"]);
        assert_eq!(disks[0].used, 50 * GIB);
        assert_eq!(disks[0].device, "/dev/nvme0n1p2");
        assert!((disks[1].frac() - 0.9).abs() < 1e-6);
        assert_eq!(read_disks(&MemFs::new()), None);
    }

    #[test]
    fn name_is_the_last_device_component() {
        let disk = |device: &str| Disk {
            mount: "/".into(),
            device: device.into(),
            total: 1,
            used: 0,
            avail: 1,
        };
        assert_eq!(disk("/dev/nvme0n1p2").name(), "nvme0n1p2");
        assert_eq!(disk("/dev/sda1").name(), "sda1");
        assert_eq!(disk("/dev/mapper/vg-data").name(), "vg-data");
    }

    #[test]
    fn reserved_blocks_count_as_used_space() {
        // ext4's 5% root reserve: df shows 50 / (50 + 45) = 53%.
        let d = Disk {
            mount: "/".into(),
            device: "/dev/sda1".into(),
            total: 100,
            used: 50,
            avail: 45,
        };
        assert!((d.frac() - 50.0 / 95.0).abs() < 1e-6);
    }
}
