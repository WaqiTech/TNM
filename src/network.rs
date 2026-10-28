use crate::host;
use crate::host::Counters;
use crate::record::Metric;
use std::error::Error;
use std::fs;
use std::time::Instant;

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

fn read_protocol_counters(path: &str) -> Result<Counters> {
    let content = fs::read_to_string(path)?;
    let mut lines = content.lines();
    let mut values = Counters::new();
    while let (Some(names), Some(numbers)) = (lines.next(), lines.next()) {
        let mut names = names.split_whitespace();
        let prefix = names.next().unwrap_or("").trim_end_matches(':');
        for (key, value) in names.zip(numbers.split_whitespace().skip(1)) {
            if let Ok(value) = value.parse() {
                values.insert(format!("{prefix}.{key}"), value);
            }
        }
    }
    Ok(values)
}

fn conntrack() -> Counters {
    let Ok(content) = fs::read_to_string("/proc/net/stat/nf_conntrack") else {
        return Counters::new();
    };
    let mut lines = content.lines();
    let names: Vec<_> = lines.next().unwrap_or("").split_whitespace().collect();
    let mut values = Counters::new();
    for line in lines {
        for (key, value) in names.iter().zip(line.split_whitespace()) {
            if let Ok(value) = u64::from_str_radix(value, 16) {
                *values.entry((*key).into()).or_default() += value;
            }
        }
    }
    values
}

pub(crate) struct Network {
    protocols: Counters,
    tcp_ext: Counters,
    conntrack: Counters,
    last_sample: Instant,
}

impl Network {
    pub(crate) fn try_new() -> Result<Self> {
        Ok(Self {
            protocols: read_protocol_counters("/proc/net/snmp")?,
            tcp_ext: read_protocol_counters("/proc/net/netstat")?,
            conntrack: conntrack(),
            last_sample: Instant::now(),
        })
    }
    pub(crate) fn collect(
        &mut self,
        include_tcp_states: bool,
        batch: &mut Vec<Metric>,
    ) -> Result<()> {
        let now = Instant::now();
        let elapsed = now
            .duration_since(self.last_sample)
            .as_secs_f64()
            .max(0.001);
        let current = read_protocol_counters("/proc/net/snmp")?;
        for (field, name) in [
            ("Tcp.ActiveOpens", "tcp_conn_active"),
            ("Tcp.PassiveOpens", "tcp_conn_passive"),
            ("Tcp.AttemptFails", "tcp_attempt_fails"),
            ("Udp.InDatagrams", "udp_in_datagrams"),
            ("Udp.OutDatagrams", "udp_out_datagrams"),
            ("Udp.InErrors", "udp_in_errors"),
            ("Udp.RcvbufErrors", "udp_rcvbu_errors"),
            ("Udp.SndbufErrors", "udp_sndbuf_errors"),
            ("Ip.ReasmFails", "ip_reasm_fails"),
        ] {
            if let Some(value) = current.get(field) {
                batch.push(Metric::unit(
                    name,
                    (host::rounded(
                        host::delta(*value, host::counter(&self.protocols, field)) as f64 / elapsed,
                    ))
                    .into(),
                    "count",
                ));
            }
        }
        if let Some(value) = current.get("Tcp.CurrEstab") {
            batch.push(Metric::unit("tcp_curr_estab", (*value).into(), "count"));
        }
        self.protocols = current;
        let tcp_counters = read_protocol_counters("/proc/net/netstat")?;
        for (field, name) in [
            ("TCPTimeouts", "tcp_timeouts"),
            ("TCPFastRetrans", "tcp_fastretrans"),
            ("ListenDrops", "tcp_listendrops"),
            ("ListenOverflows", "tcp_listenoverflows"),
            ("TCPReqQFullDrop", "tcp_reqqfulldrop"),
            ("PAWSPassive", "tcp_tw_recycle"),
        ] {
            let key = format!("TcpExt.{field}");
            if let Some(value) = tcp_counters.get(&key) {
                batch.push(Metric::unit(
                    name,
                    (host::delta(*value, host::counter(&self.tcp_ext, &key))).into(),
                    "count",
                ));
            }
        }
        self.tcp_ext = tcp_counters;
        let conntrack_counters = conntrack();
        for (field, name) in [
            ("drop", "nfct_drop"),
            ("insert_failed", "nfct_insert_fail"),
            ("invalid", "nfct_invalid"),
        ] {
            if let Some(value) = conntrack_counters.get(field) {
                batch.push(Metric::unit(
                    name,
                    (host::delta(*value, host::counter(&self.conntrack, field))).into(),
                    "count",
                ));
            }
        }
        self.conntrack = conntrack_counters;
        let sock = fs::read_to_string("/proc/net/sockstat")?;
        if let Some(line) = sock.lines().find(|line| line.starts_with("TCP:")) {
            let values: Vec<_> = line.split_whitespace().collect();
            if let Some(i) = values.iter().position(|v| *v == "tw") {
                batch.push(Metric::unit(
                    "tcp_time_await",
                    (values
                        .get(i + 1)
                        .ok_or("incomplete TCP sockstat")?
                        .parse::<u64>()?)
                    .into(),
                    "count",
                ));
            }
        }
        if include_tcp_states {
            let mut states = Counters::new();
            for path in ["/proc/net/tcp", "/proc/net/tcp6"] {
                let Ok(content) = fs::read_to_string(path) else {
                    continue;
                };
                for line in content.lines().skip(1) {
                    if let Some(state) = line.split_whitespace().nth(3) {
                        *states.entry(state.into()).or_default() += 1;
                    }
                }
            }
            for (state, name) in [
                ("08", "tcp_close_wait"),
                ("0A", "tcp_listen"),
                ("02", "tcp_syn_sent"),
                ("03", "tcp_syn_rcvd"),
                ("04", "tcp_fin_wait_1"),
                ("05", "tcp_fin_wait_2"),
            ] {
                batch.push(Metric::unit(
                    name,
                    (host::counter(&states, state)).into(),
                    "count",
                ));
            }
        }
        self.last_sample = now;
        Ok(())
    }
}
