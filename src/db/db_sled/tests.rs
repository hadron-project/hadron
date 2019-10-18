use super::{
    *,
    raft::*,
};
use crate::{
    app::{AppData, RgEntry},
    auth::UserRole,
    db::models::{Stream, StreamEntry, StreamType, StreamVisibility},
    proto::client,
};
use actix_raft::messages::{Entry, EntryPayload, EntryNormal, MembershipConfig};
use std::sync::Arc;
use proptest::prelude::*;
use tempfile;
use uuid;

proptest! {
    #[test]
    fn unchecked_u64_from_be_bytes_should_succeed_for_all_be_u64_bytes(val in u64::min_value()..u64::max_value()) {
        let u64_bytes = sled::IVec::from(&val.to_be_bytes());
        let output = unchecked_u64_from_be_bytes(u64_bytes);
        assert_eq!(output, val);
    }
}

impl SledStorage {
    /// Get a handle to the database.
    pub fn db(&self) -> sled::Db {
        self.db.clone()
    }

    /// Get a handle to the Raft log tree.
    pub fn log(&self) -> sled::Tree {
        self.log.clone()
    }
}

mod sled_storage {
    use super::*;

    #[test]
    fn index_users_data_should_return_empty_index_with_no_data() {
        let (_dir, db) = setup_db();
        let output = SledStorage::index_users_data(&db).expect("index users data");
        assert_eq!(output.len(), 0);
    }

    #[test]
    fn index_users_data_should_return_expected_index_with_populated_data() {
        let (_dir, db) = setup_db();
        let users_index = setup_base_users(&db);
        let output = SledStorage::index_users_data(&db).expect("index users data");
        assert_eq!(output, users_index);
        assert_eq!(output.len(), users_index.len());
        assert_eq!(output.len(), 3);
    }

    #[test]
    fn index_tokens_data_should_return_empty_index_with_no_data() {
        let (_dir, db) = setup_db();
        let output = SledStorage::index_tokens_data(&db).expect("index tokens data");
        assert_eq!(output.len(), 0);
    }

    #[test]
    fn index_tokens_data_should_return_expected_index_with_populated_data() {
        let (_dir, db) = setup_db();
        let tokens_index = setup_base_tokens(&db);
        let output = SledStorage::index_tokens_data(&db).expect("index tokens data");
        assert_eq!(output, tokens_index);
        assert_eq!(output.len(), tokens_index.len());
        assert_eq!(output.len(), 3);
    }

    #[test]
    fn index_ns_data_should_return_empty_index_with_no_data() {
        let (_dir, db) = setup_db();
        let output = SledStorage::index_ns_data(&db).expect("index ns data");
        assert_eq!(output.len(), 0);
    }

    #[test]
    fn index_ns_data_should_return_expected_index_with_populated_data() {
        let (_dir, db) = setup_db();
        let ns_index = setup_base_namespaces(&db);
        let output = SledStorage::index_ns_data(&db).expect("index ns data");
        assert_eq!(output, ns_index);
        assert_eq!(output.len(), ns_index.len());
        assert_eq!(output.len(), 3);
    }

    #[test]
    fn index_endpoints_data_should_return_empty_index_with_no_data() {
        let (_dir, db) = setup_db();
        let output = SledStorage::index_endpoints_data(&db).expect("index endpoints data");
        assert_eq!(output.len(), 0);
    }

    #[test]
    fn index_endpoints_data_should_return_expected_index_with_populated_data() {
        let (_dir, db) = setup_db();
        let endpoints_index = setup_base_endpoints(&db);
        let output = SledStorage::index_endpoints_data(&db).expect("index endpoints data");
        assert_eq!(output, endpoints_index);
        assert_eq!(output.len(), endpoints_index.len());
        assert_eq!(output.len(), 3);
    }

    #[test]
    fn index_pipelines_data_should_return_empty_index_with_no_data() {
        let (_dir, db) = setup_db();
        let output = SledStorage::index_pipelines_data(&db).expect("index pipelines data");
        assert_eq!(output.len(), 0);
    }

    #[test]
    fn index_pipelines_data_should_return_expected_index_with_populated_data() {
        let (_dir, db) = setup_db();
        let pipelines_index = setup_base_pipelines(&db);
        let output = SledStorage::index_pipelines_data(&db).expect("index pipelines data");
        assert_eq!(output, pipelines_index);
        assert_eq!(output.len(), pipelines_index.len());
        assert_eq!(output.len(), 3);
    }

    #[test]
    fn index_streams_data_should_return_empty_index_with_no_data() {
        let (_dir, db) = setup_db();
        let streams_tree = db.open_tree(consts::OBJECTS_STREAMS).expect("open streams path prefix");
        let output = SledStorage::index_streams_data(&db, &streams_tree).expect("index streams data");
        assert_eq!(output.len(), 0);
    }

    #[test]
    fn index_streams_data_should_return_expected_index_with_populated_data() {
        let (_dir, db) = setup_db();
        let streams_tree = db.open_tree(consts::OBJECTS_STREAMS).expect("open streams path prefix");
        let streams_index = setup_base_streams(&db, &streams_tree);
        let output = SledStorage::index_streams_data(&db, &streams_tree).expect("index streams data");
        assert_eq!(output.len(), streams_index.len());
        assert_eq!(output.len(), 2); // TODO: change this back to 3 once we have indexed streams.
        for (s0, s1) in output.values().zip(streams_index.values()) {
            assert_eq!(s0.stream, s1.stream);
        }
    }

    #[test]
    fn stream_keyspace_data_returns_expected_value() {
        let (ns, name) = ("default", "slipstream");
        let expected = format!("/streams/default/slipstream/data");
        let output = SledStorage::stream_keyspace_data(ns, name);
        assert_eq!(expected, output);
    }

    #[test]
    fn stream_keyspace_metadata_returns_expected_value() {
        let (ns, name) = ("default", "slipstream");
        let expected = format!("/streams/default/slipstream/metadata");
        let output = SledStorage::stream_keyspace_metadata(ns, name);
        assert_eq!(expected, output);
    }

    //////////////////////////////////////////////////////////////////////
    // Handle AppendLogEntry /////////////////////////////////////////////

    #[test]
    fn handle_append_log_entry() {
        let mut sys = System::builder().name("test").stop_on_panic(true).build();
        let dir = tempfile::tempdir_in("/tmp").expect("new temp dir");
        let db_path = dir.path().join("db").to_string_lossy().to_string();
        let storage = SledStorage::new(&db_path).expect("instantiate storage");
        let log = storage.log();
        let storage_addr = storage.start();
        let entry = Entry{term: 20, index: 99999, payload: EntryPayload::Normal(EntryNormal{data: AppData::from(client::PubStreamRequest{
            namespace: String::from("default"), stream: String::from("events"), payload: vec![],
        })})};
        let msg = RgAppendEntryToLog::new(Arc::new(entry.clone()));

        let f = storage_addr.send(msg).map_err(|err| panic!("MailboxError: {}", err)).and_then(|res| res).map_err(|err| panic!("ClientError {:?}", err));
        sys.block_on(f).expect("sys run");

        // Ensure the expected data was written to disk.
        let entries: Vec<_> = log.iter()
            .map(|res| res.expect("iter log entry"))
            .map(|(_, raw)| bincode::deserialize::<RgEntry>(&raw).expect("deserialize entry"))
            .collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].index, entry.index);
        assert_eq!(entries[0].term, entry.term);
        match &entries[0].payload {
            EntryPayload::Normal(entry) => match &entry.data {
                AppData::PubStream(data) => {
                    assert_eq!(data.namespace.as_str(), "default");
                    assert_eq!(data.stream.as_str(), "events");
                    assert_eq!(data.payload.len(), 0);
                }
                _ => panic!("expected a populated PubStreamRequest entry"),
            }
            _ => panic!("unexpected entry type"),
        }
    }

    //////////////////////////////////////////////////////////////////////
    // Handle ReplicateLogEntries ////////////////////////////////////////

    #[test]
    fn handle_get_log_entries() {
        let mut sys = System::builder().name("test").stop_on_panic(true).build();
        let dir = tempfile::tempdir_in("/tmp").expect("new temp dir");
        let db_path = dir.path().join("db").to_string_lossy().to_string();
        let storage = SledStorage::new(&db_path).expect("instantiate storage");
        let log = storage.log();
        let storage_addr = storage.start();
        let entry0 = Entry{term: 1, index: 0, payload: EntryPayload::Normal(EntryNormal{data: AppData::from(client::PubStreamRequest{
            namespace: String::from("default"), stream: String::from("events0"), payload: vec![],
        })})};
        log.insert(entry0.index.to_be_bytes(), bincode::serialize(&entry0).expect("serialize entry")).expect("append to log");
        let entry1 = Entry{term: 1, index: 1, payload: EntryPayload::Normal(EntryNormal{data: AppData::from(client::PubStreamRequest{
            namespace: String::from("default"), stream: String::from("events1"), payload: vec![],
        })})};
        log.insert(entry1.index.to_be_bytes(), bincode::serialize(&entry1).expect("serialize entry")).expect("append to log");
        let msg = RgGetLogEntries::new(0, 500);

        let f = storage_addr.send(msg).map_err(|err| panic!(err)).and_then(|res| res).map_err(|err| panic!(err))
            .map(|entries| {
                assert_eq!(entries.len(), 2);
                assert_eq!(entries[0].index, 0);
                assert_eq!(entries[1].index, 1);
                assert_eq!(entries[0].term, 1);
                assert_eq!(entries[1].term, 1);
            });
        sys.block_on(f).expect("sys run");
    }

    //////////////////////////////////////////////////////////////////////
    // Handle ReplicateLogEntries ////////////////////////////////////////

    #[test]
    fn handle_replicate_log_entries() {
        let mut sys = System::builder().name("test").stop_on_panic(true).build();
        let dir = tempfile::tempdir_in("/tmp").expect("new temp dir");
        let db_path = dir.path().join("db").to_string_lossy().to_string();
        let storage = SledStorage::new(&db_path).expect("instantiate storage");
        let log = storage.log();
        let storage_addr = storage.start();
        let msg0 = Entry{term: 1, index: 0, payload: EntryPayload::Normal(EntryNormal{data: AppData::from(client::PubStreamRequest{
            namespace: String::from("default"), stream: String::from("events0"), payload: vec![],
        })})};
        let msg1 = Entry{term: 1, index: 1, payload: EntryPayload::Normal(EntryNormal{data: AppData::from(client::PubStreamRequest{
            namespace: String::from("default"), stream: String::from("events1"), payload: vec![],
        })})};
        let msg = RgReplicateToLog::new(Arc::new(vec![msg0.clone(), msg1.clone()]));

        let f = storage_addr.send(msg).map_err(|err| panic!(err)).and_then(|res| res).map_err(|err| panic!(err));
        sys.block_on(f).expect("sys run");

        // Ensure the expected data was written to disk.
        let entries: Vec<_> = log.iter()
            .map(|res| res.expect("iter log entry"))
            .map(|(_, raw)| bincode::deserialize::<RgEntry>(&raw).expect("deserialize entry"))
            .collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].index, msg0.index);
        assert_eq!(entries[0].term, msg0.term);
        match &entries[0].payload {
            EntryPayload::Normal(entry) => match &entry.data {
                AppData::PubStream(data) => {
                    assert_eq!(data.namespace.as_str(), "default");
                    assert_eq!(data.stream.as_str(), "events0");
                    assert_eq!(data.payload.len(), 0);
                }
                _ => panic!("expected a populated PubStreamRequest entry"),
            }
            _ => panic!("unexpected entry type"),
        }
        assert_eq!(entries[1].index, msg1.index);
        assert_eq!(entries[1].term, msg1.term);
        match &entries[1].payload {
            EntryPayload::Normal(entry) => match &entry.data {
                AppData::PubStream(data) => {
                    assert_eq!(data.namespace.as_str(), "default");
                    assert_eq!(data.stream.as_str(), "events1");
                    assert_eq!(data.payload.len(), 0);
                }
                _ => panic!("expected a populated PubStreamRequest entry"),
            }
            _ => panic!("unexpected entry type"),
        }
    }

    //////////////////////////////////////////////////////////////////////
    // Handle SaveHardState //////////////////////////////////////////////

    #[test]
    fn handle_save_hard_state() {
        let mut sys = System::builder().name("test").stop_on_panic(true).build();
        let dir = tempfile::tempdir_in("/tmp").expect("new temp dir");
        let db_path = dir.path().join("db").to_string_lossy().to_string();
        let storage = SledStorage::new(&db_path).expect("instantiate storage");
        let db = storage.db();
        let storage_addr = storage.start();
        let orig_hs = HardState{
            current_term: 666, voted_for: Some(6),
            membership: MembershipConfig{is_in_joint_consensus: false, members: vec![6], non_voters: Vec::new(), removing: Vec::new()},
        };
        let msg = RgSaveHardState::new(orig_hs.clone());

        let f = storage_addr.send(msg).map_err(|err| panic!(err)).and_then(|res| res).map_err(|err| panic!(err));
        sys.block_on(f).expect("sys run");

        // Ensure the expected data was written to disk.
        let raw_hs = db.get(consts::RAFT_HARDSTATE_KEY).expect("get hardstate from disk").expect("hardstate value should exist");
        let hs: HardState = bincode::deserialize(&raw_hs).expect("deserialize hardstate");
        assert_eq!(orig_hs.current_term, hs.current_term);
        assert_eq!(orig_hs.voted_for, hs.voted_for);
        assert_eq!(orig_hs.membership, hs.membership);
    }
}

//////////////////////////////////////////////////////////////////////////
// Fixtures //////////////////////////////////////////////////////////////

fn setup_db() -> (tempfile::TempDir, sled::Db) {
    let dir = tempfile::tempdir_in("/tmp").expect("new temp dir");
    let dbpath = dir.path().join("db");
    let tree = sled::Db::open(&dbpath).expect("open database");
    (dir, tree)
}

fn setup_base_users(db: &sled::Db) -> BTreeMap<String, User> {
    let mut index = BTreeMap::new();
    let user0 = User{name: String::from("root-0"), role: UserRole::Root};
    let user1 = User{name: String::from("admin-0"), role: UserRole::Admin{namespaces: vec![String::from("default")]}};
    let user2 = User{name: String::from("viewer-0"), role: UserRole::Viewer{namespaces: vec![String::from("default")], metrics: true}};
    index.insert(user0.name.clone(), user0);
    index.insert(user1.name.clone(), user1);
    index.insert(user2.name.clone(), user2);

    for user in index.values() {
        let user_bytes = bincode::serialize(&user).expect("serialize user");
        db.insert(&user.name, user_bytes.as_slice()).expect("write user to disk");
    }

    index
}

fn setup_base_tokens(db: &sled::Db) -> BTreeSet<String> {
    let mut index = BTreeSet::new();
    index.insert(uuid::Uuid::new_v4().to_string());
    index.insert(uuid::Uuid::new_v4().to_string());
    index.insert(uuid::Uuid::new_v4().to_string());

    for token in index.iter() {
        db.insert(token.as_bytes(), token.as_bytes()).expect("write token to disk");
    }

    index
}

fn setup_base_namespaces(db: &sled::Db) -> BTreeSet<String> {
    let mut index = BTreeSet::new();
    index.insert(String::from("default"));
    index.insert(String::from("railgun"));
    index.insert(String::from("rg"));

    for ns in index.iter() {
        db.insert(ns.as_bytes(), ns.as_bytes()).expect("write namespace to disk");
    }

    index
}

fn setup_base_endpoints(db: &sled::Db) -> BTreeSet<String> {
    let mut index = BTreeSet::new();
    let ns = "identity-service";
    index.insert(format!("{}/{}", ns, "sign-up"));
    index.insert(format!("{}/{}", ns, "login"));
    index.insert(format!("{}/{}", ns, "reset-password"));

    for val in index.iter() {
        let keybytes = val.as_bytes();
        db.insert(keybytes, keybytes).expect("write endpoint to disk");
    }

    index
}

fn setup_base_pipelines(db: &sled::Db) -> BTreeMap<String, Pipeline> {
    let mut index = BTreeMap::new();
    let ns = String::from("identity-service");
    let pipe0 = Pipeline{namespace: ns.clone(), name: String::from("sign-up")};
    let pipe1 = Pipeline{namespace: ns.clone(), name: String::from("login")};
    let pipe2 = Pipeline{namespace: ns.clone(), name: String::from("reset-password")};
    index.insert(format!("{}/{}", &pipe0.namespace, &pipe0.name), pipe0);
    index.insert(format!("{}/{}", &pipe1.namespace, &pipe1.name), pipe1);
    index.insert(format!("{}/{}", &pipe2.namespace, &pipe2.name), pipe2);

    for (key, pipeline) in index.iter() {
        let pipebytes = bincode::serialize(pipeline).expect("serialize pipeline");
        let keybytes = key.as_bytes();
        db.insert(keybytes, pipebytes).expect("write pipeline to disk");
    }

    index
}

fn setup_base_streams(db: &sled::Db, tree: &sled::Tree) -> BTreeMap<String, StreamWrapper> {
    let mut index = BTreeMap::new();
    let stream_name = String::from("events");

    // Setup some indexed IDs.
    let mut indexed_ids = BTreeSet::new();
    indexed_ids.insert(String::from("testing"));

    let stream0 = Stream{namespace: String::from("identity-service"), name: stream_name.clone(), stream_type: StreamType::Standard, visibility: StreamVisibility::Namespace};
    let stream1 = Stream{namespace: String::from("projects-service"), name: stream_name.clone(), stream_type: StreamType::Standard, visibility: StreamVisibility::Private(String::from("pipelineX"))};
    // let stream2 = Stream{namespace: String::from("billing-service"), name: stream_name.clone(), stream_type: StreamType::UniqueId{index: indexed_ids}, visibility: StreamVisibility::Namespace};
    for stream in vec![stream0, stream1] {
        let keyspace_data = SledStorage::stream_keyspace_data(&stream.namespace, &stream.name);
        let keyspace_meta = SledStorage::stream_keyspace_metadata(&stream.namespace, &stream.name);
        let data_tree = db.open_tree(&keyspace_data).expect("open stream data keyspace");
        let meta_tree = db.open_tree(&keyspace_meta).expect("open stream metadata keyspace");
        index.insert(format!("{}/{}", &stream.namespace, &stream.name), StreamWrapper{stream, next_index: 0, data: data_tree, meta: meta_tree});
    }

    for (key, stream_wrapper) in index.iter() {
        let streambytes = bincode::serialize(&stream_wrapper.stream).expect("serialize stream");
        let keybytes = key.as_bytes();
        tree.insert(keybytes, streambytes).expect("write stream to disk");
    }

    // Write a single entry to stream "billing-service/events" for testing.
    let keyspace = SledStorage::stream_keyspace_data("billing-service", &stream_name);
    let entry_bytes = bincode::serialize(&StreamEntry{index: 0, data: vec![]}).expect("serialize stream entry");
    db.open_tree(&keyspace).expect("open stream keyspace").insert(0u64.to_be_bytes(), entry_bytes).expect("write stream entry");

    index
}
