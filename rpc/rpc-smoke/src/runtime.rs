//! Runtime ownership for benchmark processes; never a driver configuration.

#[allow(clippy::redundant_pub_crate)] // Binary-private helper shared by two entry points.
#[derive(clap::Args)]
pub(super) struct RuntimeArgs {
    /// Partition connections over this many independent runtimes.
    #[arg(long, default_value_t = 1)]
    pub runtime_shards: usize,
}

impl RuntimeArgs {
    pub(super) fn build(&self) -> Result<Vec<tokio::runtime::Runtime>, Box<dyn std::error::Error>> {
        let workers: usize = std::env::var("TOKIO_WORKER_THREADS")
            .unwrap_or_else(|_| {
                std::thread::available_parallelism()
                    .map_or(1, usize::from)
                    .to_string()
            })
            .parse()?;
        if self.runtime_shards == 0 || workers == 0 || !workers.is_multiple_of(self.runtime_shards)
        {
            return Err(
                "TOKIO_WORKER_THREADS must be positive and divisible by runtime-shards".into(),
            );
        }
        (0..self.runtime_shards)
            .map(|_| {
                tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(workers / self.runtime_shards)
                    .enable_all()
                    .build()
                    .map_err(Into::into)
            })
            .collect()
    }
}
