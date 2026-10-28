# TNM

A Rust agent that sends Linux metrics to Tencent's native monitoring service.

## Run

```sh
cargo build --release
TNM_UPLOAD_INTERVAL_SECS=10 ./target/release/tnm
```

Run on a Tencent Cloud Linux VM with a synchronized system clock. Instance identity and private IP are discovered automatically. Root is not required.

Use `--once` to collect and upload one sample, then exit.

## Configuration

Set environment variables to override defaults. Boolean values are `true` or `false`.

| Variable | Default |
| --- | --- |
| `TNM_SAMPLE_INTERVAL_SECS` | `10` |
| `TNM_UPLOAD_INTERVAL_SECS` | `60` |
| `TNM_HEARTBEAT_INTERVAL_SECS` | `30` |
| `TNM_EXTENDED_MEMORY_INTERVAL_SECS` | `60` |
| `TNM_PROCESS_INTERVAL_SECS` | `60` |
| `TNM_SYSTEM_INTERVAL_SECS` | `300` |
| `TNM_IDENTITY_REFRESH_INTERVAL_SECS` | `300` |
| `TNM_TIMEOUT_MS` | `3000` |
| `TNM_MAX_PENDING_SAMPLES` | `60` |
| `TNM_CPU`, `TNM_MEMORY`, `TNM_DISK`, `TNM_NETWORK`, `TNM_PROCESSES`, `TNM_SYSTEM_INFO`, `TNM_HEARTBEAT` | `true` |
| `TNM_TCP_STATES` | `false` |

| Endpoint variable | Default |
| --- | --- |
| `TNM_METRICS_URL` | `http://receiver.barad.monitor.tencentcs.com/ca_report.cgi` |
| `TNM_METRICS_FALLBACK_URL` | `http://receiver.barad.tencentyun.com/ca_report.cgi` |
| `TNM_HEARTBEAT_URL` | `http://receiver.barad.monitor.tencentcs.com/heart_report.cgi` |
| `TNM_HEARTBEAT_FALLBACK_URL` | `http://receiver.barad.tencentyun.com/heart_report.cgi` |
| `TNM_METADATA_URL` | `http://metadata.tencentyun.com/latest/meta-data/` |

Endpoints use HTTP. Proxy environment variables are ignored.

## Metrics

| Group | Metrics |
| --- | --- |
| CPU | Utilization, load averages, runnable tasks, thread count and softnet drops |
| Memory | Usage, cache, buffers, page scans and swap |
| Filesystems | Capacity, utilization and inode utilization |
| Network | TCP connections/errors, UDP/IP rates and kernel TCP/conntrack counters |
| Processes | Total, running and zombie counts; PID limit |
| System | OS, kernel and agent version |
| Heartbeat | `barad_agent_hb` |

`TNM_SAMPLE_INTERVAL_SECS` controls CPU, memory, filesystem and network sampling. NVMe devices also report disk I/O. Set `TNM_TCP_STATES=true` for detailed TCP states.

## Development

Follow the [Google Rust style guide](https://google.github.io/styleguide/rustguide.html).

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo build --release
```

## License

[MIT](LICENSE).
