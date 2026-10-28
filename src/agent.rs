use crate::collector::Collector;
use crate::config::Config;
use crate::identity::Identity;
use crate::record::Record;
use crate::transport::Transport;
use std::collections::VecDeque;
use std::error::Error;
use std::time::{Duration, Instant};
use tokio::time;

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

pub(crate) struct Agent {
    config: Config,
    transport: Transport,
    identity: Identity,
    collector: Collector,
    pending: VecDeque<Vec<Record>>,
    sample_at: Instant,
    upload_at: Instant,
    heartbeat_at: Instant,
    identity_at: Instant,
}

impl Agent {
    pub(crate) async fn try_new(config: Config) -> Result<Self> {
        let transport = Transport::new(&config);
        let identity = Identity::discover(&transport, &config).await?;
        let collector = Collector::try_new(&config)?;
        let now = Instant::now();
        Ok(Self {
            sample_at: now + Duration::from_secs(config.sample_interval_secs),
            upload_at: now + Duration::from_secs(config.upload_interval_secs),
            heartbeat_at: now + Duration::from_secs(config.heartbeat_interval_secs),
            identity_at: now + Duration::from_secs(config.identity_refresh_interval_secs),
            config,
            transport,
            identity,
            collector,
            pending: VecDeque::new(),
        })
    }

    pub(crate) async fn run_once(&mut self) -> Result<()> {
        time::sleep(Duration::from_secs(self.config.sample_interval_secs)).await;
        let records = self.collector.sample(&self.config, &self.identity)?;
        if !records.is_empty() {
            self.transport
                .upload(
                    &self.config.metrics_url,
                    &self.config.metrics_fallback_url,
                    &records,
                )
                .await?;
        }
        if self.config.heartbeat {
            self.transport
                .upload(
                    &self.config.heartbeat_url,
                    &self.config.heartbeat_fallback_url,
                    &[Record::heartbeat(&self.identity)],
                )
                .await?;
        }
        println!(
            "tnm: receiver accepted {} metric records; heartbeat {}",
            records.len(),
            if self.config.heartbeat {
                "accepted"
            } else {
                "disabled"
            }
        );
        Ok(())
    }

    pub(crate) async fn run(mut self) -> ! {
        eprintln!(
            "tnm: started; sample={}s upload={}s heartbeat={}s",
            self.config.sample_interval_secs,
            self.config.upload_interval_secs,
            self.config.heartbeat_interval_secs
        );
        loop {
            let now = Instant::now();
            if now >= self.identity_at {
                self.refresh_identity().await;
            }
            if now >= self.sample_at {
                self.collect_sample();
            }
            if now >= self.upload_at {
                self.upload_metrics().await;
            }
            if self.config.heartbeat && now >= self.heartbeat_at {
                self.upload_heartbeat().await;
            }
            let mut next = self.sample_at.min(self.upload_at).min(self.identity_at);
            if self.config.heartbeat {
                next = next.min(self.heartbeat_at);
            }
            time::sleep_until(time::Instant::from_std(next)).await;
        }
    }

    async fn refresh_identity(&mut self) {
        match Identity::discover(&self.transport, &self.config).await {
            Ok(identity) => self.identity = identity,
            Err(error) => eprintln!("tnm: identity refresh failed: {error}"),
        }
        self.identity_at =
            Instant::now() + Duration::from_secs(self.config.identity_refresh_interval_secs);
    }

    fn collect_sample(&mut self) {
        match self.collector.sample(&self.config, &self.identity) {
            Ok(records) if !records.is_empty() => {
                if self.pending.len() >= self.config.max_pending_samples {
                    self.pending.pop_front();
                    eprintln!("tnm: sample buffer full; oldest sample dropped");
                }
                self.pending.push_back(records);
            }
            Ok(_) => {}
            Err(error) => eprintln!("tnm: collection failed: {error}"),
        }
        self.sample_at = Instant::now() + Duration::from_secs(self.config.sample_interval_secs);
    }

    async fn upload_metrics(&mut self) {
        let records: Vec<Record> = self.pending.drain(..).flatten().collect();
        if !records.is_empty()
            && let Err(error) = self
                .transport
                .upload(
                    &self.config.metrics_url,
                    &self.config.metrics_fallback_url,
                    &records,
                )
                .await
        {
            eprintln!("tnm: metric upload failed; batch dropped: {error}");
        }
        self.upload_at = Instant::now() + Duration::from_secs(self.config.upload_interval_secs);
    }

    async fn upload_heartbeat(&mut self) {
        if let Err(error) = self
            .transport
            .upload(
                &self.config.heartbeat_url,
                &self.config.heartbeat_fallback_url,
                &[Record::heartbeat(&self.identity)],
            )
            .await
        {
            eprintln!("tnm: heartbeat failed: {error}");
        }
        self.heartbeat_at =
            Instant::now() + Duration::from_secs(self.config.heartbeat_interval_secs);
    }
}
