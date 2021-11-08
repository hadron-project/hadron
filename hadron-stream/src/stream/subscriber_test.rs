use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::sync::mpsc;

use crate::config::Config;
use crate::database::Database;
use crate::fixtures;
use crate::grpc::{StreamSubscribeSetup, StreamSubscribeSetupStartingPoint};
use crate::models::stream::Subscription;
use crate::stream::subscriber::{self, StreamSubCtlMsg, SubGroupDataCache, SubscriberInfo, SubscriptionGroup};
use crate::stream::{PREFIX_STREAM_SUBS, PREFIX_STREAM_SUB_OFFSETS};
use crate::utils;

#[tokio::test]
async fn spawn_group_fetch_ok() -> Result<()> {
    let (config, _tmpdir) = Config::new_test()?;
    let db = Database::new(config.clone()).await?;
    let stream_tree = db.get_stream_tree().await?;
    let group_name = Arc::new(String::from("test-group"));
    let (_last_ts, _last_offset) = fixtures::setup_stream_data(&stream_tree).await?;
    let ((tx, mut rx), timeout) = (mpsc::channel(1), std::time::Duration::from_secs(5));

    subscriber::spawn_group_fetch(group_name.clone(), 0, 20, stream_tree.clone(), tx.clone());
    let output_msg = tokio::time::timeout(timeout, rx.recv())
        .await
        .context("timeout from group fetch")?
        .context("error from spawn group fetch")?;
    let output = match output_msg {
        StreamSubCtlMsg::FetchStreamRecords(data) => data?,
        res => anyhow::bail!("unexpected response type from spawn_group_fetch, expected FetchStreamRecords, got {:?}", res),
    };

    assert_eq!(&output.group_name, &group_name, "mismatch group names, expected {}, got {}", &group_name, &output.group_name);
    assert_eq!(
        output.last_included_offset,
        Some(19),
        "mismatch last_included_offset, expected {:?}, got {:?}",
        output.last_included_offset,
        Some(19),
    );
    assert_eq!(output.data.len(), 20, "mismatch payload len, expected {}, got {}", 20, output.data.len());

    Ok(())
}

#[tokio::test]
async fn spawn_group_fetch_empty_response_when_no_new_data() -> Result<()> {
    let (config, _tmpdir) = Config::new_test()?;
    let db = Database::new(config.clone()).await?;
    let stream_tree = db.get_stream_tree().await?;
    let group_name = Arc::new(String::from("test-group"));
    let (_last_ts, last_offset) = fixtures::setup_stream_data(&stream_tree).await?;
    let ((tx, mut rx), timeout) = (mpsc::channel(1), std::time::Duration::from_secs(5));

    subscriber::spawn_group_fetch(group_name.clone(), last_offset + 1, 20, stream_tree.clone(), tx.clone());
    let output_msg = tokio::time::timeout(timeout, rx.recv())
        .await
        .context("timeout from group fetch")?
        .context("error from spawn group fetch")?;
    let output = match output_msg {
        StreamSubCtlMsg::FetchStreamRecords(data) => data?,
        res => anyhow::bail!("unexpected response type from spawn_group_fetch, expected FetchStreamRecords, got {:?}", res),
    };

    assert_eq!(&output.group_name, &group_name, "mismatch group names, expected {}, got {}", &group_name, &output.group_name);
    assert!(
        output.last_included_offset.is_none(),
        "mismatch last_included_offset, expected None, got {:?}",
        output.last_included_offset
    );
    assert!(output.data.is_empty(), "mismatch payload len, expected data to be empty, got len {}", output.data.len());

    Ok(())
}

#[tokio::test]
async fn try_record_delivery_response_ok_no_update_with_error_response() -> Result<()> {
    let (config, _tmpdir) = Config::new_test()?;
    let db = Database::new(config.clone()).await?;
    let stream_tree = db.get_stream_tree().await?;
    let group_name = Arc::new(String::from("test-group"));
    let err_msg = String::from("test error message");

    let _output = subscriber::try_record_delivery_response(Err(err_msg), group_name.clone(), stream_tree.clone())
        .await
        .context("error from try_record_delivery_response")?;

    let key = utils::ivec_from_iter(PREFIX_STREAM_SUB_OFFSETS.iter().copied().chain(group_name.as_bytes().iter().copied()));
    let opt_sub_offset = stream_tree.get(key).context("error fetching stream sub offset")?;
    assert!(opt_sub_offset.is_none(), "expected stream subscription offset to be None, got {:?}", opt_sub_offset);

    Ok(())
}

#[tokio::test]
async fn try_record_delivery_response_ok_with_expected_stream_record() -> Result<()> {
    let (config, _tmpdir) = Config::new_test()?;
    let db = Database::new(config.clone()).await?;
    let stream_tree = db.get_stream_tree().await?;
    let group_name = Arc::new(String::from("test-group"));
    let last_offset_processed = 100;

    let _output = subscriber::try_record_delivery_response(Ok(last_offset_processed), group_name.clone(), stream_tree.clone())
        .await
        .context("error from try_record_delivery_response")?;

    let key = utils::ivec_from_iter(PREFIX_STREAM_SUB_OFFSETS.iter().copied().chain(group_name.as_bytes().iter().copied()));
    let sub_offset_ivec = stream_tree
        .get(key)
        .context("error fetching stream sub offset")?
        .context("expected stream subscription offset to be recorded, got None")?;
    let sub_offset = utils::decode_u64(&sub_offset_ivec)?;
    assert_eq!(
        sub_offset, last_offset_processed,
        "expected stream subscription offset to be {}, got {}",
        last_offset_processed, sub_offset
    );

    Ok(())
}

#[tokio::test]
async fn ensure_subscriber_record_ok_new_record_offset_beginning() -> Result<()> {
    let (config, _tmpdir) = Config::new_test()?;
    let db = Database::new(config.clone()).await?;
    let stream_tree = db.get_stream_tree().await?;
    let (group_name, current_offset, max_batch_size) = (String::from("test-group"), 100, 500);
    let setup = StreamSubscribeSetup {
        group_name: group_name.clone(),
        durable: true,
        max_batch_size,
        starting_point: Some(StreamSubscribeSetupStartingPoint::Beginning(Default::default())),
    };
    let mut subs = SubscriberInfo::default();

    let output = subscriber::ensure_subscriber_record(&stream_tree, &setup, &mut subs, current_offset)
        .await
        .context("error ensuring subscriber record")?;

    assert_eq!(output.group_name.as_str(), group_name.as_str(), "expected group name {}, got {}", group_name, output.group_name);
    assert!(output.durable, "expected subscription group to be durable, got false");
    assert_eq!(output.offset, 0, "expected subscription group offset to be 0, got {}", output.offset);
    assert!(output.active_channels.is_empty(), "expected subscription active channels to be initially empty");
    assert!(matches!(output.delivery_cache, SubGroupDataCache::None), "expected subscription active channels to be initially empty");
    assert!(!output.is_fetching_data, "expected subscription to not be fetching data initially");

    let sub_model_key = utils::ivec_from_iter(PREFIX_STREAM_SUBS.iter().copied().chain(group_name.as_bytes().iter().copied()));
    let sub_model_ivec = stream_tree
        .get(sub_model_key)
        .context("error fetching stream subscription")?
        .context("expected subscription to be recorded")?;
    let sub_model: Subscription = utils::decode_model(&sub_model_ivec).context("error decoding sub_model_ivec")?;
    assert_eq!(sub_model.group_name, group_name, "expected group name {}, got {}", group_name, sub_model.group_name);
    assert_eq!(sub_model.max_batch_size, max_batch_size, "expected group name {}, got {}", max_batch_size, sub_model.max_batch_size);
    assert_eq!(
        output.subscription, sub_model,
        "expected subscription models to be identical, expected {:?}, got {:?}",
        sub_model, output.subscription
    );

    let sub_offset_key = utils::ivec_from_iter(PREFIX_STREAM_SUB_OFFSETS.iter().copied().chain(group_name.as_bytes().iter().copied()));
    let sub_offset_ivec = stream_tree
        .get(sub_offset_key)
        .context("error fetching stream subscription offset")?
        .context("expected subscription offset to be recorded")?;
    let sub_offset = utils::decode_u64(&sub_offset_ivec).context("error decoding sub_offset_ivec")?;
    assert_eq!(sub_offset, 0, "expected new sub offset to be 0, got {}", sub_offset);

    Ok(())
}

#[tokio::test]
async fn ensure_subscriber_record_ok_noop_if_already_exists() -> Result<()> {
    let (config, _tmpdir) = Config::new_test()?;
    let db = Database::new(config.clone()).await?;
    let stream_tree = db.get_stream_tree().await?;
    let (group_name, current_offset, max_batch_size) = (String::from("test-group"), 100, 500);
    let setup = StreamSubscribeSetup {
        group_name: group_name.clone(),
        durable: true,
        max_batch_size,
        starting_point: Some(StreamSubscribeSetupStartingPoint::Beginning(Default::default())),
    };
    let mut subs = SubscriberInfo::default();
    subs.groups.insert(
        group_name.clone(),
        SubscriptionGroup {
            group_name: Arc::new(String::from("something-else-test")),
            subscription: Default::default(),
            durable: true,
            offset: 0,
            active_channels: Default::default(),
            delivery_cache: SubGroupDataCache::None,
            is_fetching_data: false,
        },
    );

    let output = subscriber::ensure_subscriber_record(&stream_tree, &setup, &mut subs, current_offset)
        .await
        .context("error ensuring subscriber record")?;

    assert_ne!(output.group_name.as_str(), group_name.as_str(), "expected group names to be different to assert no-op behavior");

    let sub_model_key = utils::ivec_from_iter(PREFIX_STREAM_SUBS.iter().copied().chain(group_name.as_bytes().iter().copied()));
    let sub_model_opt = stream_tree.get(sub_model_key).context("error fetching stream subscription")?;
    assert!(sub_model_opt.is_none(), "expected sub_model_opt to be None");

    let sub_offset_key = utils::ivec_from_iter(PREFIX_STREAM_SUB_OFFSETS.iter().copied().chain(group_name.as_bytes().iter().copied()));
    let sub_offset_opt = stream_tree.get(sub_offset_key).context("error fetching stream subscription offset")?;
    assert!(sub_offset_opt.is_none(), "expected sub_offset_opt to be None");

    Ok(())
}

#[tokio::test]
async fn ensure_subscriber_record_ok_start_point_latest() -> Result<()> {
    let (config, _tmpdir) = Config::new_test()?;
    let db = Database::new(config.clone()).await?;
    let stream_tree = db.get_stream_tree().await?;
    let (group_name, current_offset, max_batch_size) = (String::from("test-group"), 100, 500);
    let setup = StreamSubscribeSetup {
        group_name: group_name.clone(),
        durable: false,
        max_batch_size,
        starting_point: Some(StreamSubscribeSetupStartingPoint::Latest(Default::default())),
    };
    let mut subs = SubscriberInfo::default();

    let output = subscriber::ensure_subscriber_record(&stream_tree, &setup, &mut subs, current_offset)
        .await
        .context("error ensuring subscriber record")?;

    assert_eq!(output.group_name.as_str(), group_name.as_str(), "expected group name {}, got {}", group_name, output.group_name);
    assert!(!output.durable, "expected subscription group to be durable, got false");
    assert_eq!(output.offset, current_offset, "expected subscription group offset to be {}, got {}", current_offset, output.offset);
    assert!(output.active_channels.is_empty(), "expected subscription active channels to be initially empty");
    assert!(matches!(output.delivery_cache, SubGroupDataCache::None), "expected subscription active channels to be initially empty");
    assert!(!output.is_fetching_data, "expected subscription to not be fetching data initially");

    let sub_model_key = utils::ivec_from_iter(PREFIX_STREAM_SUBS.iter().copied().chain(group_name.as_bytes().iter().copied()));
    let sub_model_opt = stream_tree.get(sub_model_key).context("error fetching stream subscription")?;
    assert!(sub_model_opt.is_none(), "expected sub_model_opt to be None");

    let sub_offset_key = utils::ivec_from_iter(PREFIX_STREAM_SUB_OFFSETS.iter().copied().chain(group_name.as_bytes().iter().copied()));
    let sub_offset_opt = stream_tree.get(sub_offset_key).context("error fetching stream subscription offset")?;
    assert!(sub_offset_opt.is_none(), "expected sub_offset_opt to be None");

    Ok(())
}

#[tokio::test]
async fn ensure_subscriber_record_ok_start_point_offset() -> Result<()> {
    let (config, _tmpdir) = Config::new_test()?;
    let db = Database::new(config.clone()).await?;
    let stream_tree = db.get_stream_tree().await?;
    let (group_name, current_offset, max_batch_size, start_offset) = (String::from("test-group"), 100, 500, 50);
    let setup = StreamSubscribeSetup {
        group_name: group_name.clone(),
        durable: false,
        max_batch_size,
        starting_point: Some(StreamSubscribeSetupStartingPoint::Offset(start_offset)),
    };
    let mut subs = SubscriberInfo::default();

    let output = subscriber::ensure_subscriber_record(&stream_tree, &setup, &mut subs, current_offset)
        .await
        .context("error ensuring subscriber record")?;

    assert_eq!(output.group_name.as_str(), group_name.as_str(), "expected group name {}, got {}", group_name, output.group_name);
    assert!(!output.durable, "expected subscription group to be durable, got false");
    assert_eq!(output.offset, start_offset - 1, "expected subscription group offset to be {}, got {}", start_offset - 1, output.offset);
    assert!(output.active_channels.is_empty(), "expected subscription active channels to be initially empty");
    assert!(matches!(output.delivery_cache, SubGroupDataCache::None), "expected subscription active channels to be initially empty");
    assert!(!output.is_fetching_data, "expected subscription to not be fetching data initially");

    let sub_model_key = utils::ivec_from_iter(PREFIX_STREAM_SUBS.iter().copied().chain(group_name.as_bytes().iter().copied()));
    let sub_model_opt = stream_tree.get(sub_model_key).context("error fetching stream subscription")?;
    assert!(sub_model_opt.is_none(), "expected sub_model_opt to be None");

    let sub_offset_key = utils::ivec_from_iter(PREFIX_STREAM_SUB_OFFSETS.iter().copied().chain(group_name.as_bytes().iter().copied()));
    let sub_offset_opt = stream_tree.get(sub_offset_key).context("error fetching stream subscription offset")?;
    assert!(sub_offset_opt.is_none(), "expected sub_offset_opt to be None");

    Ok(())
}
