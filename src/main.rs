mod agent;
mod collector;
mod config;
mod host;
mod identity;
mod network;
mod record;
mod storage;
mod transport;

use crate::agent::Agent;
use crate::config::Config;
use std::env;
use std::error::Error;
use std::process;
use tokio::runtime::Builder;

fn run() -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut once = false;
    let arguments = env::args().skip(1);
    for argument in arguments {
        match argument.as_str() {
            "--once" => once = true,
            "--version" => {
                println!("tnm {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--help" => {
                println!(
                    "tnm [--once]\nSettings: TNM_* environment variables.\n--once samples once, verifies uploads, then exits."
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }
    let config = Config::from_env()?;
    Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()?
        .block_on(async {
            let mut agent = Agent::try_new(config).await?;
            if once {
                agent.run_once().await
            } else {
                agent.run().await
            }
        })
}

fn main() {
    if let Err(error) = run() {
        eprintln!("tnm: {error}");
        process::exit(1);
    }
}
