use crate::config::Config;
use crate::host::{Cpu, Memory};
use crate::identity::Identity;
use crate::network::Network;
use crate::record;
use crate::record::{Metric, Record};
use crate::storage::Storage;
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::time::Instant;

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

pub(crate) struct Collector {
    cpu: Option<Cpu>,
    memory: Option<Memory>,
    network: Option<Network>,
    storage: Option<Storage>,
    last_extended: Option<Instant>,
    last_processes: Option<Instant>,
    last_system: Option<Instant>,
}
impl Collector {
    pub(crate) fn try_new(config: &Config) -> Result<Self> {
        Ok(Self {
            cpu: config.cpu.then(Cpu::try_new).transpose()?,
            memory: config.memory.then(Memory::try_new).transpose()?,
            network: config.network.then(Network::try_new).transpose()?,
            storage: config.disk.then(Storage::try_new).transpose()?,
            last_extended: None,
            last_processes: None,
            last_system: None,
        })
    }
    pub(crate) fn sample(&mut self, config: &Config, identity: &Identity) -> Result<Vec<Record>> {
        let now = Instant::now();
        let extended = due(
            self.last_extended,
            now,
            config.extended_memory_interval_secs,
        );
        let mut metrics = Vec::new();
        if let Some(cpu) = &mut self.cpu {
            cpu.collect(&mut metrics)?;
        }
        if let Some(memory) = &mut self.memory {
            memory.collect(extended, &mut metrics)?;
        }
        if let Some(network) = &mut self.network {
            network.collect(config.tcp_states, &mut metrics)?;
        }
        if config.processes && due(self.last_processes, now, config.process_interval_secs) {
            collect_processes(&mut metrics)?;
            self.last_processes = Some(now);
        }
        let mut records = Vec::new();
        if !metrics.is_empty() {
            records.push(Record::metrics(identity, metrics));
        }
        if let Some(storage) = &mut self.storage {
            records.extend(storage.collect(identity)?);
        }
        if config.system_info && due(self.last_system, now, config.system_interval_secs) {
            records.push(collect_system_info(identity)?);
            self.last_system = Some(now);
        }
        if extended {
            self.last_extended = Some(now);
        }
        Ok(records)
    }
}
fn due(last: Option<Instant>, now: Instant, interval_secs: u64) -> bool {
    last.is_none_or(|last| now.duration_since(last).as_secs() >= interval_secs)
}

fn collect_processes(batch: &mut Vec<Metric>) -> Result<()> {
    let (mut total, mut zombies, mut running) = (0, 0, 0);
    for entry in fs::read_dir("/proc")?.flatten() {
        if !entry
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|b| b.is_ascii_digit())
        {
            continue;
        }
        total += 1;
        if let Ok(stat) = fs::read_to_string(entry.path().join("stat")) {
            match stat.rsplit_once(") ").and_then(|(_, r)| r.chars().next()) {
                Some('Z') => zombies += 1,
                Some('R') => running += 1,
                _ => {}
            }
        }
    }
    batch.extend([
        Metric::unit("process_num", total.into(), "count"),
        Metric::unit("zombie_num", zombies.into(), "count"),
        Metric::unit("activeproc_num", running.into(), "count"),
        Metric::unit(
            "maxproc_num",
            (fs::read_to_string("/proc/sys/kernel/pid_max")?
                .trim()
                .parse::<u64>()?)
            .into(),
            "count",
        ),
    ]);
    Ok(())
}

fn collect_system_info(identity: &Identity) -> Result<Record> {
    let os = fs::read_to_string("/etc/os-release")?
        .lines()
        .find_map(|s| s.strip_prefix("PRETTY_NAME="))
        .unwrap_or("unknown")
        .trim_matches('"')
        .to_lowercase();
    let kernel = fs::read_to_string("/proc/sys/kernel/osrelease")?
        .trim()
        .to_lowercase();
    let dimension = BTreeMap::from([
        ("vm_uuid".into(), identity.uuid.clone()),
        (
            "barad_version".into(),
            concat!("tnm-", env!("CARGO_PKG_VERSION")).into(),
        ),
        ("os_version".into(), os),
        ("kernel_version".into(), kernel),
    ]);
    Ok(Record {
        namespace: "qce/cvm",
        dimension,
        timestamp: record::timestamp(),
        batch: vec![Metric::unit(
            "barad_agent_linux_verion_info",
            0.into(),
            "count",
        )],
    })
}
