use crate::config::Config;
use crate::transport::Transport;
use hyper::Uri;
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio::net::{self, UdpSocket};
use tokio::time;

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

pub(crate) struct Identity {
    pub(crate) uuid: String,
    ip: Ipv4Addr,
}

impl Identity {
    pub(crate) async fn discover(transport: &Transport, config: &Config) -> Result<Self> {
        let metadata_url = format!("{}/uuid", config.metadata_url.trim_end_matches('/'));
        let metadata = transport
            .get_text(&metadata_url, 1024)
            .await
            .ok()
            .filter(|value| valid_uuid(value.trim()));
        let uuid = match metadata {
            Some(uuid) => uuid,
            None => read_local_uuid().ok_or("cloud UUID unavailable")?,
        };
        let ip = time::timeout(Duration::from_millis(config.timeout_ms), async {
            match route_ip(&config.metrics_url).await {
                Ok(ip) => Ok(ip),
                Err(_) => route_ip(&config.metrics_fallback_url).await,
            }
        })
        .await??;
        Ok(Self {
            uuid: uuid.trim().to_lowercase(),
            ip,
        })
    }

    pub(crate) fn dimensions(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("vm_uuid".into(), self.uuid.clone()),
            ("vmip".into(), self.ip.to_string()),
        ])
    }
}

fn read_local_uuid() -> Option<String> {
    for path in ["/sys/class/dmi/id/product_serial", "/etc/uuid"] {
        if let Ok(value) = fs::read_to_string(path)
            && valid_uuid(value.trim())
        {
            return Some(value);
        }
    }
    None
}

fn valid_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

async fn route_ip(url: &str) -> Result<Ipv4Addr> {
    let uri: Uri = url.parse()?;
    let host = uri.host().ok_or("receiver hostname missing")?;
    let address = net::lookup_host((host, uri.port_u16().unwrap_or(80)))
        .await?
        .find(SocketAddr::is_ipv4)
        .ok_or("receiver has no IPv4 address")?;
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
    socket.connect(address).await?;
    match socket.local_addr()?.ip() {
        IpAddr::V4(ip) => Ok(ip),
        IpAddr::V6(_) => Err("route is not IPv4".into()),
    }
}
