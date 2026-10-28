use hyper::Uri;
use std::env;
use std::error::Error;

pub(crate) struct Config {
    pub(crate) sample_interval_secs: u64,
    pub(crate) upload_interval_secs: u64,
    pub(crate) heartbeat_interval_secs: u64,
    pub(crate) extended_memory_interval_secs: u64,
    pub(crate) process_interval_secs: u64,
    pub(crate) system_interval_secs: u64,
    pub(crate) identity_refresh_interval_secs: u64,
    pub(crate) timeout_ms: u64,
    pub(crate) max_pending_samples: usize,
    pub(crate) metrics_url: String,
    pub(crate) metrics_fallback_url: String,
    pub(crate) heartbeat_url: String,
    pub(crate) heartbeat_fallback_url: String,
    pub(crate) metadata_url: String,
    pub(crate) cpu: bool,
    pub(crate) memory: bool,
    pub(crate) disk: bool,
    pub(crate) network: bool,
    pub(crate) processes: bool,
    pub(crate) system_info: bool,
    pub(crate) heartbeat: bool,
    pub(crate) tcp_states: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            sample_interval_secs: 10,
            upload_interval_secs: 60,
            heartbeat_interval_secs: 30,
            extended_memory_interval_secs: 60,
            process_interval_secs: 60,
            system_interval_secs: 300,
            identity_refresh_interval_secs: 300,
            timeout_ms: 3000,
            max_pending_samples: 60,
            metrics_url: "http://receiver.barad.monitor.tencentcs.com/ca_report.cgi".into(),
            metrics_fallback_url: "http://receiver.barad.tencentyun.com/ca_report.cgi".into(),
            heartbeat_url: "http://receiver.barad.monitor.tencentcs.com/heart_report.cgi".into(),
            heartbeat_fallback_url: "http://receiver.barad.tencentyun.com/heart_report.cgi".into(),
            metadata_url: "http://metadata.tencentyun.com/latest/meta-data/".into(),
            cpu: true,
            memory: true,
            disk: true,
            network: true,
            processes: true,
            system_info: true,
            heartbeat: true,
            tcp_states: false,
        }
    }
}

impl Config {
    pub(crate) fn from_env() -> Result<Self, Box<dyn Error + Send + Sync>> {
        let mut config = Self::default();
        for (name, value) in env::vars_os() {
            let Some(name) = name.to_str().filter(|name| name.starts_with("TNM_")) else {
                continue;
            };
            let value = value
                .to_str()
                .ok_or_else(|| format!("{name} must be UTF-8"))?;
            let number = match name {
                "TNM_SAMPLE_INTERVAL_SECS" => Some(&mut config.sample_interval_secs),
                "TNM_UPLOAD_INTERVAL_SECS" => Some(&mut config.upload_interval_secs),
                "TNM_HEARTBEAT_INTERVAL_SECS" => Some(&mut config.heartbeat_interval_secs),
                "TNM_EXTENDED_MEMORY_INTERVAL_SECS" => {
                    Some(&mut config.extended_memory_interval_secs)
                }
                "TNM_PROCESS_INTERVAL_SECS" => Some(&mut config.process_interval_secs),
                "TNM_SYSTEM_INTERVAL_SECS" => Some(&mut config.system_interval_secs),
                "TNM_IDENTITY_REFRESH_INTERVAL_SECS" => {
                    Some(&mut config.identity_refresh_interval_secs)
                }
                "TNM_TIMEOUT_MS" => Some(&mut config.timeout_ms),
                _ => None,
            };
            if let Some(number) = number {
                *number = positive_number(name, value)?;
                continue;
            }
            let flag = match name {
                "TNM_CPU" => Some(&mut config.cpu),
                "TNM_MEMORY" => Some(&mut config.memory),
                "TNM_DISK" => Some(&mut config.disk),
                "TNM_NETWORK" => Some(&mut config.network),
                "TNM_PROCESSES" => Some(&mut config.processes),
                "TNM_SYSTEM_INFO" => Some(&mut config.system_info),
                "TNM_HEARTBEAT" => Some(&mut config.heartbeat),
                "TNM_TCP_STATES" => Some(&mut config.tcp_states),
                _ => None,
            };
            if let Some(flag) = flag {
                *flag = value
                    .parse()
                    .map_err(|_| format!("{name} must be true or false"))?;
                continue;
            }
            match name {
                "TNM_MAX_PENDING_SAMPLES" => {
                    config.max_pending_samples = usize::try_from(positive_number(name, value)?)?;
                }
                "TNM_METRICS_URL" => config.metrics_url = value.into(),
                "TNM_METRICS_FALLBACK_URL" => config.metrics_fallback_url = value.into(),
                "TNM_HEARTBEAT_URL" => config.heartbeat_url = value.into(),
                "TNM_HEARTBEAT_FALLBACK_URL" => config.heartbeat_fallback_url = value.into(),
                "TNM_METADATA_URL" => config.metadata_url = value.into(),
                _ => return Err(format!("unknown setting: {name}").into()),
            }
        }
        for (name, url) in [
            ("TNM_METRICS_URL", &config.metrics_url),
            ("TNM_METRICS_FALLBACK_URL", &config.metrics_fallback_url),
            ("TNM_HEARTBEAT_URL", &config.heartbeat_url),
            ("TNM_HEARTBEAT_FALLBACK_URL", &config.heartbeat_fallback_url),
            ("TNM_METADATA_URL", &config.metadata_url),
        ] {
            let uri: Uri = url
                .parse()
                .map_err(|_| format!("{name} must be an absolute http:// URL"))?;
            if uri.scheme_str() != Some("http") || uri.host().is_none() {
                return Err(format!("{name} must be an absolute http:// URL").into());
            }
        }
        Ok(config)
    }
}

fn positive_number(name: &str, value: &str) -> Result<u64, Box<dyn Error + Send + Sync>> {
    let value = value
        .parse()
        .map_err(|_| format!("{name} must be a positive integer"))?;
    if value == 0 {
        return Err(format!("{name} must be a positive integer").into());
    }
    Ok(value)
}
