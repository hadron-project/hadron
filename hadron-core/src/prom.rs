use anyhow::Result;

// pub const METRIC_CPU_SECONDS_TOTAL: &str = "process_cpu_seconds_total";
// pub const METRIC_START_TIME_SECONDS: &str = "process_start_time_seconds";
pub const METRIC_OPEN_FDS: &str = "process_open_fds";
pub const METRIC_MAX_FDS: &str = "process_max_fds";
pub const METRIC_VIRTUAL_MEMORY_BYTES: &str = "process_virtual_memory_bytes";
pub const METRIC_VIRTUAL_MEMORY_MAX_BYTES: &str = "process_virtual_memory_max_bytes";
pub const METRIC_RESIDENT_MEMORY_BYTES: &str = "process_resident_memory_bytes";
pub const METRIC_HEAP_BYTES: &str = "process_heap_bytes";
pub const METRIC_THREADS: &str = "process_threads";

/// Register the Prometheus recommended process metrics.
///
/// This function should be called only once, early in the lifetime of the process.
pub fn register_proc_metrics() {
    // metrics::register_counter!(METRIC_START_TIME_SECONDS, metrics::Unit::Count, "Start time of the process since unix epoch in seconds.");
    // metrics::register_counter!(METRIC_CPU_SECONDS_TOTAL, metrics::Unit::Count, "Total user and system CPU time spent in seconds.");
    metrics::register_gauge!(METRIC_OPEN_FDS, metrics::Unit::Count, "Number of open file descriptors.");
    metrics::register_gauge!(METRIC_MAX_FDS, metrics::Unit::Count, "Maximum number of open file descriptors.");
    metrics::register_gauge!(METRIC_VIRTUAL_MEMORY_BYTES, metrics::Unit::Bytes, "Virtual memory size in bytes.");
    metrics::register_gauge!(METRIC_VIRTUAL_MEMORY_MAX_BYTES, metrics::Unit::Bytes, "Maximum amount of virtual memory available in bytes.");
    metrics::register_gauge!(METRIC_RESIDENT_MEMORY_BYTES, metrics::Unit::Bytes, "Resident memory size in bytes.");
    metrics::register_gauge!(METRIC_HEAP_BYTES, metrics::Unit::Bytes, "Process heap size in bytes.");
    metrics::register_gauge!(METRIC_THREADS, metrics::Unit::Count, "Number of OS threads in the process.");
}

/// Collect a sample of process metrics.
#[cfg(not(feature = "prom"))]
pub fn collect_proc_metrics() -> Result<()> {
    anyhow::bail!("metrics sampling is only configured for Linux")
}

/// Collect a sample of process metrics.
#[cfg(feature = "prom")]
pub fn collect_proc_metrics() -> Result<()> {
    use anyhow::Context;
    let proc = procfs::process::Process::myself().context("error gather process metrics")?;

    match proc.fd_count() {
        Ok(open_fds) => metrics::gauge!(METRIC_OPEN_FDS, open_fds as f64),
        Err(err) => tracing::error!(error = ?err, "error gathering metric {}", METRIC_OPEN_FDS),
    }
    match proc.limits() {
        Ok(limits) => {
            if let procfs::process::LimitValue::Value(max) = limits.max_open_files.soft_limit {
                metrics::gauge!(METRIC_MAX_FDS, max as f64);
            }
            if let procfs::process::LimitValue::Value(max) = limits.max_address_space.soft_limit {
                metrics::gauge!(METRIC_VIRTUAL_MEMORY_MAX_BYTES, max);
            }
            if let procfs::process::LimitValue::Value(max) = limits.max_data_size.soft_limit {
                metrics::gauge!(METRIC_HEAP_BYTES, max);
            }
        }
        Err(err) => tracing::error!(error = ?err, "error gathering metric {}", METRIC_MAX_FDS),
    }

    metrics::gauge!(METRIC_VIRTUAL_MEMORY_BYTES, proc.stat.vsize as f64);
    match proc.stat.rss_bytes() {
        Ok(rss) => metrics::gauge!(METRIC_RESIDENT_MEMORY_BYTES, rss as f64),
        Err(err) => tracing::error!(error = ?err, "error gathering metric {}", METRIC_RESIDENT_MEMORY_BYTES),
    }

    metrics::gauge!(METRIC_THREADS, proc.stat.num_threads as f64);
    Ok(())
}

/// Spawn a process metrics sampler which will shutdown when the given `shutdown` future resolves.
pub fn spawn_proc_metrics_sampler(shutdown: impl std::future::Future<Output = ()> + Send + 'static) -> tokio::task::JoinHandle<()> {
    if cfg!(feature = "prom") {
        tokio::spawn(async move {
            let mut sample_interval = tokio::time::interval(std::time::Duration::from_secs(5));
            tokio::pin!(shutdown);
            loop {
                tokio::select! {
                    _ = sample_interval.tick() => match collect_proc_metrics() {
                        Ok(_) => continue,
                        Err(err) => tracing::error!(error = ?err, "error collecting process metrics sample"),
                    },
                    _ = &mut shutdown => break,
                }
            }
        })
    } else {
        tokio::spawn(async move {})
    }
}
