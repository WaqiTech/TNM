use crate::record::Metric;
use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::time::Instant;

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
pub(crate) type Counters = HashMap<String, u64>;

pub(crate) fn read_counters(path: &str) -> Result<Counters> {
    let content = fs::read_to_string(path)?;
    Ok(content
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            Some((
                parts.next()?.trim_end_matches(':').to_owned(),
                parts.next()?.parse().ok()?,
            ))
        })
        .collect())
}

pub(crate) fn counter(values: &Counters, key: &str) -> u64 {
    values.get(key).copied().unwrap_or(0)
}

pub(crate) fn delta(now: u64, prev: u64) -> u64 {
    now.saturating_sub(prev)
}

pub(crate) fn rounded(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

struct CpuTimes {
    total: u64,
    idle_and_wait: u64,
}

fn read_cpu_times() -> Result<CpuTimes> {
    let content = fs::read_to_string("/proc/stat")?;
    let values: Vec<u64> = content
        .lines()
        .next()
        .ok_or("empty /proc/stat")?
        .split_whitespace()
        .skip(1)
        .take(8)
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()?;
    if values.len() < 5 {
        return Err("incomplete CPU counters".into());
    }
    Ok(CpuTimes {
        total: values.iter().sum(),
        idle_and_wait: values[3] + values[4],
    })
}

struct LoadAverage {
    one_minute: f64,
    five_minutes: f64,
    fifteen_minutes: f64,
    running_tasks: u64,
    total_threads: u64,
}

fn parse_load_average(content: &str) -> Result<LoadAverage> {
    let mut fields = content.split_whitespace();
    let one_minute = fields.next().ok_or("missing one-minute load")?.parse()?;
    let five_minutes = fields.next().ok_or("missing five-minute load")?.parse()?;
    let fifteen_minutes = fields
        .next()
        .ok_or("missing fifteen-minute load")?
        .parse()?;
    let (running_tasks, total_threads) = fields
        .next()
        .ok_or("missing load task counts")?
        .split_once('/')
        .ok_or("invalid load task counts")?;
    Ok(LoadAverage {
        one_minute,
        five_minutes,
        fifteen_minutes,
        running_tasks: running_tasks.parse()?,
        total_threads: total_threads.parse()?,
    })
}

fn cpu_utilization(current: &CpuTimes, previous: &CpuTimes) -> Option<f64> {
    let total = delta(current.total, previous.total);
    let idle = delta(current.idle_and_wait, previous.idle_and_wait);
    (total > 0).then(|| rounded(100.0 * total.saturating_sub(idle) as f64 / total as f64))
}

fn softnet_drops() -> u64 {
    fs::read_to_string("/proc/net/softnet_stat")
        .unwrap_or_default()
        .lines()
        .filter_map(|line| u64::from_str_radix(line.split_whitespace().nth(1)?, 16).ok())
        .sum()
}

pub(crate) struct Cpu {
    counters: CpuTimes,
    softnet_drops: u64,
    last_sample: Instant,
}

impl Cpu {
    pub(crate) fn try_new() -> Result<Self> {
        Ok(Self {
            counters: read_cpu_times()?,
            softnet_drops: softnet_drops(),
            last_sample: Instant::now(),
        })
    }
    pub(crate) fn collect(&mut self, batch: &mut Vec<Metric>) -> Result<()> {
        let now = Instant::now();
        let elapsed = now
            .duration_since(self.last_sample)
            .as_secs_f64()
            .max(0.001);
        let current = read_cpu_times()?;
        let load = parse_load_average(&fs::read_to_string("/proc/loadavg")?)?;
        if let Some(utilization) = cpu_utilization(&current, &self.counters) {
            batch.push(Metric::new("cpu_usage", utilization.into()));
        }
        batch.extend([
            Metric::new("cpu_load_1", load.one_minute.into()),
            Metric::new("cpu_load_5", load.five_minutes.into()),
            Metric::new("cpu_load_15", load.fifteen_minutes.into()),
            Metric::new("cpu_queue_length", load.running_tasks.into()),
            Metric::new("total_threads", load.total_threads.into()),
        ]);
        let drops = softnet_drops();
        batch.push(Metric::new(
            "softirq_package_lost",
            (rounded(delta(drops, self.softnet_drops) as f64 / elapsed)).into(),
        ));
        self.counters = current;
        self.softnet_drops = drops;
        self.last_sample = now;
        Ok(())
    }
}

pub(crate) struct Memory {
    counters: Counters,
    page_size: u64,
    last_sample: Instant,
}

impl Memory {
    pub(crate) fn try_new() -> Result<Self> {
        Ok(Self {
            counters: read_counters("/proc/vmstat")?,
            page_size: page_size()?,
            last_sample: Instant::now(),
        })
    }
    pub(crate) fn collect(&mut self, extended: bool, batch: &mut Vec<Metric>) -> Result<()> {
        let now = Instant::now();
        let elapsed = now
            .duration_since(self.last_sample)
            .as_secs_f64()
            .max(0.001);
        let memory = read_counters("/proc/meminfo")?;
        let total = counter(&memory, "MemTotal");
        if total == 0 {
            return Err("MemTotal missing".into());
        }
        let cached = counter(&memory, "Cached") + counter(&memory, "SReclaimable");
        let available = (counter(&memory, "MemFree") + counter(&memory, "Buffers") + cached)
            .saturating_sub(counter(&memory, "Shmem"))
            .min(total);
        let modern_available = memory
            .get("MemAvailable")
            .copied()
            .filter(|n| *n > 0)
            .unwrap_or(available);
        batch.extend([
            Metric::unit(
                "mem_usage",
                (rounded(100.0 * total.saturating_sub(modern_available) as f64 / total as f64))
                    .into(),
                "mib",
            ),
            Metric::unit(
                "mem_used",
                (total.saturating_sub(available) >> 10).into(),
                "mib",
            ),
            Metric::unit(
                "mem_total_used",
                ((total >> 10).saturating_sub(counter(&memory, "MemFree") >> 10)).into(),
                "mib",
            ),
        ]);
        let virtual_memory = read_counters("/proc/vmstat")?;
        let pages = |v: &Counters| {
            v.iter()
                .filter(|(key, _)| key.starts_with("pgscan_"))
                .map(|(_, v)| *v)
                .sum()
        };
        batch.push(Metric::unit(
            "mem_pagescan",
            (delta(pages(&virtual_memory), pages(&self.counters))).into(),
            "count",
        ));
        if extended {
            for (name, value) in [
                ("mem_total", total),
                ("mem_cached", cached),
                ("mem_buffers", counter(&memory, "Buffers")),
                ("swap_total", counter(&memory, "SwapTotal")),
                (
                    "swap_used",
                    counter(&memory, "SwapTotal").saturating_sub(counter(&memory, "SwapFree")),
                ),
            ] {
                batch.push(Metric::unit(name, (value >> 10).into(), "mib"));
            }
            for (name, field) in [("disk_swap_in", "pswpin"), ("disk_swap_out", "pswpout")] {
                batch.push(Metric::unit(
                    name,
                    (rounded(
                        delta(
                            counter(&virtual_memory, field),
                            counter(&self.counters, field),
                        ) as f64
                            * self.page_size as f64
                            / 1_048_576.0
                            / elapsed,
                    ))
                    .into(),
                    "mps",
                ));
            }
            if let Some(value) = memory.get("HardwareCorrupted") {
                batch.push(Metric::new(
                    "memory_error_status",
                    (u8::from(*value > 0)).into(),
                ));
            }
        }
        self.counters = virtual_memory;
        self.last_sample = now;
        Ok(())
    }
}

// std does not expose the OS page size; keep sysconf behind a safe wrapper.
fn page_size() -> Result<u64> {
    // SAFETY: sysconf accepts this fixed selector and has no pointer arguments.
    let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if size <= 0 {
        return Err("cannot determine the OS page size".into());
    }
    Ok(size as u64)
}
