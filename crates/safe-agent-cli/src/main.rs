mod audit;
mod cli;
mod config;
mod policy;
mod secrets;
mod session;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    cli::run(cli::Cli::parse())
}
