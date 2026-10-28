use crate::host;
use crate::identity::Identity;
use crate::record::{Metric, Record};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::ffi::CString;
use std::fs;
use std::mem::MaybeUninit;
use std::path::{Path, PathBuf};
use std::time::Instant;

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

pub(crate) struct Storage {
    counters: HashMap<String, Vec<u64>>,
    last_sample: Instant,
}
impl Storage {
    pub(crate) fn try_new() -> Result<Self> {
        Ok(Self {
            counters: disk_counters()?,
            last_sample: Instant::now(),
        })
    }
    pub(crate) fn collect(&mut self, identity: &Identity) -> Result<Vec<Record>> {
        let now = Instant::now();
        let elapsed = now
            .duration_since(self.last_sample)
            .as_secs_f64()
            .max(0.001);
        let mut records = collect_filesystems(identity)?;
        let current = disk_counters()?;
        for (name, stats) in &current {
            if !name.starts_with("nvme")
                || Path::new(&format!("/sys/class/block/{name}/partition")).exists()
            {
                continue;
            }
            let Some(prev) = self.counters.get(name) else {
                continue;
            };
            if stats.len() < 10 || prev.len() < 10 {
                continue;
            }
            let reads = host::delta(stats[0], prev[0]);
            let writes = host::delta(stats[4], prev[4]);
            let count = reads + writes;
            let busy = host::delta(stats[9], prev[9]) as f64;
            let mut row = Record::metrics(
                identity,
                vec![
                    Metric::unit(
                        "vm_disk_read_traffic",
                        (host::rounded(host::delta(stats[2], prev[2]) as f64 / 2.0 / elapsed))
                            .into(),
                        "kbps",
                    ),
                    Metric::unit(
                        "vm_disk_write_traffic",
                        (host::rounded(host::delta(stats[6], prev[6]) as f64 / 2.0 / elapsed))
                            .into(),
                        "kbps",
                    ),
                    Metric::unit(
                        "vm_disk_read_iops",
                        (host::rounded(reads as f64 / elapsed)).into(),
                        "countps",
                    ),
                    Metric::unit(
                        "vm_disk_write_iops",
                        (host::rounded(writes as f64 / elapsed)).into(),
                        "countps",
                    ),
                    Metric::new(
                        "vm_disk_tmpio",
                        (if count == 0 {
                            0.0
                        } else {
                            host::rounded(busy / count as f64)
                        })
                        .into(),
                    ),
                    Metric::new(
                        "vm_disk_util",
                        (host::rounded((busy / elapsed / 10.0).min(100.0))).into(),
                    ),
                    Metric::unit(
                        "vm_disk_await",
                        (if count == 0 {
                            0.0
                        } else {
                            host::rounded(
                                (host::delta(stats[3], prev[3]) + host::delta(stats[7], prev[7]))
                                    as f64
                                    / count as f64,
                            )
                        })
                        .into(),
                        "ms",
                    ),
                ],
            );
            row.dimension.insert("diskname".into(), name.to_lowercase());
            if let Ok(address) =
                fs::read_to_string(format!("/sys/class/block/{name}/device/address"))
            {
                row.dimension
                    .insert("address".into(), address.trim().to_lowercase());
            }
            records.push(row);
        }
        self.counters = current;
        self.last_sample = now;
        Ok(records)
    }
}

fn disk_counters() -> Result<HashMap<String, Vec<u64>>> {
    Ok(fs::read_to_string("/proc/diskstats")?
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            parts.next()?;
            parts.next()?;
            let name = parts.next()?.to_owned();
            let values = parts
                .map(str::parse)
                .collect::<std::result::Result<Vec<_>, _>>()
                .ok()?;
            Some((name, values))
        })
        .collect())
}

fn unescape_mount(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

fn collect_filesystems(identity: &Identity) -> Result<Vec<Record>> {
    let mounts = fs::read_to_string("/proc/self/mountinfo")?;
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    for line in mounts.lines() {
        let Some((left, right)) = line.split_once(" - ") else {
            continue;
        };
        let left: Vec<_> = left.split_whitespace().collect();
        let right: Vec<_> = right.split_whitespace().collect();
        if left.len() < 6
            || right.len() < 3
            || !right[1].starts_with("/dev/")
            || !seen.insert(left[2])
        {
            continue;
        }
        let mount = PathBuf::from(unescape_mount(left[4]));
        let Some(stat) = filesystem_stats(&mount)? else {
            continue;
        };
        if stat.f_blocks == 0 {
            continue;
        }
        let total = stat.f_blocks as f64 * stat.f_frsize as f64;
        let used = (stat.f_blocks - stat.f_bfree) as f64 * stat.f_frsize as f64;
        let available = stat.f_bavail as f64 * stat.f_frsize as f64;
        let name = fs::canonicalize(format!("/sys/dev/block/{}", left[2]))
            .ok()
            .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
            .unwrap_or_else(|| {
                Path::new(right[1])
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            });
        let block = PathBuf::from(format!("/sys/class/block/{name}"));
        let physical = if block.join("partition").exists() {
            fs::canonicalize(&block)
                .ok()
                .and_then(|p| {
                    p.parent()
                        .and_then(|p| p.file_name())
                        .map(|s| s.to_string_lossy().into_owned())
                })
                .unwrap_or_else(|| name.clone())
        } else {
            name.clone()
        };
        let serial = fs::read_to_string(format!("/sys/class/block/{physical}/serial"))
            .or_else(|_| fs::read_to_string(format!("/sys/class/block/{physical}/device/serial")))
            .unwrap_or_default();
        let serial = serial.trim();
        let disk_id = if serial.is_empty() {
            format!("disk-{}-{name}", identity.uuid)
        } else if physical != name {
            format!("{serial}-{name}")
        } else {
            serial.into()
        };
        let inode_used = stat.f_files.saturating_sub(stat.f_ffree);
        let inode_usage = if stat.f_files > 0 {
            inode_used as f64 / stat.f_files as f64 * 100.0
        } else {
            0.0
        };
        let usage = if used + available > 0.0 {
            used / (used + available) * 100.0
        } else {
            0.0
        };
        let mut row = Record::metrics(
            identity,
            vec![
                Metric::unit("disk_total", ((total / 1_048_576.0) as u64).into(), "mib"),
                Metric::new("disk_usage", (host::rounded(usage)).into()),
                Metric::new("inode_usage", (host::rounded(inode_usage)).into()),
            ],
        );
        row.dimension.insert("diskname".into(), name.to_lowercase());
        row.dimension
            .insert("diskid".into(), disk_id.to_lowercase());
        row.dimension
            .insert("disktype".into(), right[0].to_lowercase());
        rows.push(row);
    }
    Ok(rows)
}

// std has no filesystem-capacity API. Keep the statvfs FFI at this read-only boundary.
fn filesystem_stats(path: &Path) -> Result<Option<libc::statvfs>> {
    use std::os::unix::ffi::OsStrExt;

    let path = CString::new(path.as_os_str().as_bytes())?;
    let mut result = MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: path is NUL-terminated and alive for the call; result points to enough
    // writable, aligned storage for statvfs. Success initializes the complete struct.
    if unsafe { libc::statvfs(path.as_ptr(), result.as_mut_ptr()) } != 0 {
        return Ok(None);
    }
    // SAFETY: statvfs returned success, so result is fully initialized.
    Ok(Some(unsafe { result.assume_init() }))
}
