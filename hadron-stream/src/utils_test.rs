use anyhow::{Context, Result};

use crate::config::Config;
use crate::database::Database;
use crate::utils;

const ERR_MSG_ITER: &str = "error iterating scanned data";
const NUM_ENTRIES: u64 = 1_001;
const PREFIX_A: &[u8; 1] = b"a";
/// We use this in tests as it is middle in lexicographical sort order.
const PREFIX_B: &[u8; 1] = b"b";
const PREFIX_C: &[u8; 1] = b"c";

#[tokio::test]
async fn test_exhaustive_scan_prefix_and_range_behavior() -> Result<()> {
    let (config, _tmpdir) = Config::new_test()?;
    let db = Database::new(config.clone()).await?;
    let tree = db.get_stream_tree().await?;

    // Load data distributed across three key prefixes which are used to assert correctness of
    // range scans and prefix scans, which depend upon the correctness of key encoding.
    load_data(&tree)?;

    // Assert that prefix scan finds the correct amount of data.
    let mut count = 0;
    for kv_res in tree.scan_prefix(PREFIX_B) {
        let (key, val) = kv_res.context(ERR_MSG_ITER)?;
        if key[0] != PREFIX_B[0] {
            println!("bad key prefix: got {}; expected: {};", key[0], PREFIX_B[0]);
        } else {
            count += 1;
        }
        let _key = utils::decode_u64(&key[1..])?;
        let _val = utils::decode_u64(&val)?;
    }
    assert_eq!(count, NUM_ENTRIES, "expected scan_prefix to find {} entries, got {}", NUM_ENTRIES, count);

    // Assert that range scans preserve sort order based on our key prefix strategy.
    let (start, stop, mut count, mut current_offset) = (PREFIX_B, PREFIX_C, 0, 0u64);
    for kv_res in tree.range::<_, std::ops::Range<&[u8]>>(start..stop) {
        let (key, val) = kv_res.context(ERR_MSG_ITER)?;
        if key[0] != PREFIX_B[0] {
            println!("bad key prefix: got {}; expected: {};", key[0], &PREFIX_B[0]);
        } else {
            count += 1;
        }
        let key = utils::decode_u64(&key[1..])?;
        let val = utils::decode_u64(&val)?;
        assert_eq!(key, current_offset, "db.range with prefix iterated out of order, expected key {} got {}", current_offset, key);
        assert_eq!(val, current_offset, "db.range with prefix iterated out of order, expected val {} got {}", current_offset, val);
        current_offset += 1;
    }
    assert_eq!(count, NUM_ENTRIES, "expected range to find {} entries, got {}", NUM_ENTRIES, count);

    Ok(())
}

#[test]
fn test_ivec_and_encoding_compat() {
    let i0 = utils::ivec_from_iter(PREFIX_B.iter().copied().chain(u64::MIN.to_be_bytes().iter().copied()));
    let i1 = utils::ivec_from_iter(PREFIX_B.iter().copied().chain(1u64.to_be_bytes().iter().copied()));
    let i2 = utils::ivec_from_iter(PREFIX_B.iter().copied().chain(10u64.to_be_bytes().iter().copied()));
    let i3 = utils::ivec_from_iter(PREFIX_B.iter().copied().chain(utils::encode_u64(10)).chain("stage_name".as_bytes().iter().copied()));

    let e0 = utils::encode_byte_prefix(PREFIX_B, u64::MIN);
    let e1 = utils::encode_byte_prefix(PREFIX_B, 1u64);
    let e2 = utils::encode_byte_prefix(PREFIX_B, 10u64);

    assert_eq!(&i0, &e0, "ivec slice i0 is different from encoded slice:\n{:?}\n{:?}", &i0, &e0);
    assert_eq!(&i1, &e1, "ivec slice i1 is different from encoded slice:\n{:?}\n{:?}", &i1, &e1);
    assert_eq!(&i2, &e2, "ivec slice i2 is different from encoded slice:\n{:?}\n{:?}", &i2, &e2);
    assert_eq!(&i3[..9], &e2, "the first 9 bytes of i3 do not match the byte encoded prefix:\n{:?}\n{:?}", &i3[..9], &e2);
    assert_eq!(
        &i3[9..],
        b"stage_name",
        "the last 10 bytes of i3 do not match the expected stage name:\n{:?}\n{:?}",
        &i3[9..],
        b"stage_name"
    );
}

fn load_data(db: &sled::Tree) -> Result<()> {
    for prefix in [PREFIX_A, PREFIX_B, PREFIX_C] {
        let mut batch = sled::Batch::default();
        for offset in 0..NUM_ENTRIES {
            let key = utils::encode_byte_prefix(prefix, offset);
            batch.insert(&key, &utils::encode_u64(offset));
        }
        db.apply_batch(batch).context("error inserting data")?;
    }
    db.flush().context("error flusing data")?;
    Ok(())
}
