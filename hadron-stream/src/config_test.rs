use anyhow::Result;

use super::*;
use hadron_core::crd::StreamRetentionPolicy;

#[test]
fn config_deserializes_from_full_env() -> Result<()> {
    let config: Config = envy::from_iter(vec![
        ("RUST_LOG".into(), "error".into()),
        ("CLIENT_PORT".into(), "7000".into()),
        ("SERVER_PORT".into(), "7001".into()),
        ("NAMESPACE".into(), "default".into()),
        ("STREAM".into(), "events".into()),
        ("STATEFULSET".into(), "events".into()),
        ("POD_NAME".into(), "events-0".into()),
        ("STORAGE_DATA_PATH".into(), "/usr/local/hadron-stream/data".into()),
        ("RETENTION_POLICY_STRATEGY".into(), "time".into()),
        ("RETENTION_POLICY_RETENTION_SECONDS".into(), "604800".into()),
    ])?;

    assert!(config.rust_log == "error", "unexpected value parsed for RUST_LOG, got {}, expected {}", config.rust_log, "");
    assert!(config.client_port == 7000, "unexpected value parsed for CLIENT_PORT, got {}, expected {}", config.client_port, "7000");
    assert!(config.server_port == 7001, "unexpected value parsed for SERVER_PORT, got {}, expected {}", config.server_port, "7001");
    assert!(config.namespace == "default", "unexpected value parsed for NAMESPACE, got {}, expected {}", config.namespace, "default");
    assert!(config.stream == "events", "unexpected value parsed for STREAM, got {}, expected {}", config.stream, "events");
    assert!(
        config.statefulset == "events",
        "unexpected value parsed for STATEFULSET, got {}, expected {}",
        config.statefulset,
        "events"
    );
    assert!(config.pod_name == "events-0", "unexpected value parsed for POD_NAME, got {}, expected {}", config.pod_name, "events-0");
    assert!(config.partition == 0, "unexpected value derived for partition, got {}, expected {}", config.partition, 0);
    assert!(
        config.storage_data_path == "/usr/local/hadron-stream/data",
        "unexpected value parsed for STORAGE_DATA_PATH, got {}, expected {}",
        config.storage_data_path,
        "/usr/local/hadron-stream/data"
    );
    assert!(
        config.retention_policy.strategy == StreamRetentionPolicy::Time,
        "unexpected value parsed for RETENTION_POLICY_STRATEGY, got {}, expected {}",
        config.retention_policy.strategy,
        StreamRetentionPolicy::Time
    );
    assert!(
        config.retention_policy.retention_seconds == Some(604800),
        "unexpected value parsed for RETENTION_POLICY_RETENTION_SECONDS, got {:?}, expected {:?}",
        config.retention_policy.retention_seconds,
        Some(604800)
    );

    Ok(())
}

#[test]
fn config_deserializes_from_sparse_env() -> Result<()> {
    let config: Config = envy::from_iter(vec![
        ("RUST_LOG".into(), "error".into()),
        ("CLIENT_PORT".into(), "7000".into()),
        ("SERVER_PORT".into(), "7001".into()),
        ("NAMESPACE".into(), "default".into()),
        ("STREAM".into(), "events".into()),
        ("STATEFULSET".into(), "events".into()),
        ("POD_NAME".into(), "events-0".into()),
        ("STORAGE_DATA_PATH".into(), "/usr/local/hadron-stream/data".into()),
        ("RETENTION_POLICY_STRATEGY".into(), "time".into()),
    ])?;

    assert!(config.rust_log == "error", "unexpected value parsed for RUST_LOG, got {}, expected {}", config.rust_log, "");
    assert!(config.client_port == 7000, "unexpected value parsed for CLIENT_PORT, got {}, expected {}", config.client_port, "7000");
    assert!(config.server_port == 7001, "unexpected value parsed for SERVER_PORT, got {}, expected {}", config.server_port, "7001");
    assert!(config.namespace == "default", "unexpected value parsed for NAMESPACE, got {}, expected {}", config.namespace, "default");
    assert!(config.stream == "events", "unexpected value parsed for STREAM, got {}, expected {}", config.stream, "events");
    assert!(
        config.statefulset == "events",
        "unexpected value parsed for STATEFULSET, got {}, expected {}",
        config.statefulset,
        "events"
    );
    assert!(config.pod_name == "events-0", "unexpected value parsed for POD_NAME, got {}, expected {}", config.pod_name, "events-0");
    assert!(config.partition == 0, "unexpected value derived for partition, got {}, expected {}", config.partition, 0);
    assert!(
        config.storage_data_path == "/usr/local/hadron-stream/data",
        "unexpected value parsed for STORAGE_DATA_PATH, got {}, expected {}",
        config.storage_data_path,
        "/usr/local/hadron-stream/data"
    );
    assert!(
        config.retention_policy.strategy == StreamRetentionPolicy::Time,
        "unexpected value parsed for RETENTION_POLICY_STRATEGY, got {}, expected {}",
        config.retention_policy.strategy,
        StreamRetentionPolicy::Time
    );
    assert!(
        config.retention_policy.retention_seconds == Some(604800),
        "unexpected value parsed for RETENTION_POLICY_RETENTION_SECONDS, got {:?}, expected {:?}",
        config.retention_policy.retention_seconds,
        Some(604800)
    );

    Ok(())
}
