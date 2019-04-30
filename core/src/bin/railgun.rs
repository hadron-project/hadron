use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{str, thread};

use log::{error, info, warn};
use protobuf::Message as PbMessage;
use raft::storage::MemStorage;
use raft::{prelude::*, StateRole};
use regex::Regex;

fn main() {
    env_logger::init();
    info!("Booting railgun.");

    // Create 5 mailboxes to send/receive messages. Every node holds a `Receiver` to receive
    // messages from others, and uses the respective `Sender` to send messages to others.
    let (mut tx_vec, mut rx_vec) = (Vec::new(), Vec::new());
    for _ in 0..5 {
        let (tx, rx) = mpsc::channel();
        tx_vec.push(tx);
        rx_vec.push(rx);
    }

    // A global pending proposals queue. New proposals will be pushed back into the queue, and
    // after it's committed by the raft cluster, it will be poped from the queue.
    let proposals = Arc::new(Mutex::new(VecDeque::<Proposal>::new()));

    let mut handles = Vec::new();
    for (i, rx) in rx_vec.into_iter().enumerate() {
        // A map[peer_id -> sender]. In the example we create 5 nodes, with ids in [1, 5].
        let mailboxes = (1..6u64).zip(tx_vec.iter().cloned()).collect();
        let mut node = match i {
            // Peer 1 is the leader.
            0 => Node::create_raft_leader(1, rx, mailboxes),
            // Other peers are followers.
            _ => Node::create_raft_follower(rx, mailboxes),
        };
        let proposals = Arc::clone(&proposals);


        // Here we spawn the node on a new thread and keep a handle so we can join on them later.
        let handle = thread::spawn(move || {

            // Tick the raft node per 100ms. So use an `Instant` to trace it.
            let mut log_timer = Instant::now();
            let mut loop_timer = Instant::now();
            loop {
                thread::sleep(Duration::from_millis(10));
                loop {
                    // Step raft messages.
                    match node.my_mailbox.try_recv() {
                        Ok(msg) => node.step(msg),
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => return,
                    }
                }

                let raft_group = match node.raft_group {
                    Some(ref mut r) => r,
                    // When Node::raft_group is `None` it means the node is not initialized.
                    _ => continue,
                };

                if loop_timer.elapsed() >= Duration::from_millis(100) {
                    // Tick the raft.
                    raft_group.tick();
                    loop_timer = Instant::now();
                }

                // Let the leader pick pending proposals from the global queue.
                if raft_group.raft.state == StateRole::Leader {
                    // Handle new proposals.
                    let mut proposals = proposals.lock().unwrap();
                    for p in proposals.iter_mut().skip_while(|p| p.proposed > 0) {
                        propose(raft_group, p);
                    }

                    if log_timer.elapsed() >= Duration::from_secs(10) {
                        // Write KV data.
                        use std::fs;
                        use serde_json::to_vec_pretty;
                        fs::write("./target/state", to_vec_pretty(&node.kv_pairs).unwrap()).unwrap();
                        log_timer = Instant::now();
                    }
                }

                // Handle readies from the raft.
                on_ready(raft_group, &mut node.kv_pairs, &node.mailboxes, &proposals);
            }
        });
        handles.push(handle);
    }

    // Propose some conf changes so that followers can be initialized.
    add_all_followers(proposals.as_ref());

    // Put 100 key-value pairs.
    (0..100u16)
        .filter(|i| {
            let (proposal, rx) = Proposal::normal(*i, "hello, world".to_owned());
            proposals.lock().unwrap().push_back(proposal);
            // After we got a response from `rx`, we can assume the put succeeded and following
            // `get` operations can find the key-value pair.
            rx.recv().unwrap()
        })
        .count();

    for th in handles {
        th.join().unwrap();
    }
}

struct Node {
    // None if the raft is not initialized.
    raft_group: Option<RawNode<MemStorage>>,
    my_mailbox: Receiver<Message>,
    mailboxes: HashMap<u64, Sender<Message>>,
    // Key-value pairs after applied. `MemStorage` only contains raft logs,
    // so we need an additional storage engine.
    kv_pairs: HashMap<u16, String>,
}

impl Node {
    // Create a raft leader only with itself in its configuration.
    fn create_raft_leader(
        id: u64,
        my_mailbox: Receiver<Message>,
        mailboxes: HashMap<u64, Sender<Message>>,
    ) -> Self {
        let mut cfg = example_config();
        cfg.id = id;
        cfg.peers = vec![id];
        cfg.tag = format!("peer_{}", id);

        let storage = MemStorage::new();
        let raft_group = Some(RawNode::new(&cfg, storage, vec![]).unwrap());
        Node {
            raft_group,
            my_mailbox,
            mailboxes,
            kv_pairs: Default::default(),
        }
    }

    // Create a raft follower.
    fn create_raft_follower(
        my_mailbox: Receiver<Message>,
        mailboxes: HashMap<u64, Sender<Message>>,
    ) -> Self {
        Node {
            raft_group: None,
            my_mailbox,
            mailboxes,
            kv_pairs: Default::default(),
        }
    }

    // Initialize raft for followers.
    fn initialize_raft_from_message(&mut self, msg: &Message) {
        if !is_initial_msg(msg) {
            return;
        }
        let mut cfg = example_config();
        cfg.id = msg.get_to();
        let storage = MemStorage::new();
        self.raft_group = Some(RawNode::new(&cfg, storage, vec![]).unwrap());
    }

    // Step a raft message, initialize the raft if need.
    fn step(&mut self, msg: Message) {
        if self.raft_group.is_none() {
            if is_initial_msg(&msg) {
                self.initialize_raft_from_message(&msg);
            } else {
                return;
            }
        }
        let raft_group = self.raft_group.as_mut().unwrap();
        let _ = raft_group.step(msg);
    }
}

fn on_ready(
    raft_group: &mut RawNode<MemStorage>,
    kv_pairs: &mut HashMap<u16, String>,
    mailboxes: &HashMap<u64, Sender<Message>>,
    proposals: &Mutex<VecDeque<Proposal>>,
) {
    if !raft_group.has_ready() {
        return;
    }
    // Get the `Ready` with `RawNode::ready` interface.
    let mut ready = raft_group.ready();

    //////////////////////////////////////////////////////////////////////////
    // 1. Leader Messages & Snapshot /////////////////////////////////////////

    // If this node is the leader, then it can send out its messages first.
    // Must hear back from a majority of replicating peers before continuing.
    let is_leader = raft_group.raft.state == StateRole::Leader;
    if is_leader {
        // Send messages out peers.
        for msg in ready.messages.drain(..) {
            let to = msg.get_to();
            info!("NODE {} (leader): Sending message to peer {}: {:?}", raft_group.raft.id, to, &msg);
            if mailboxes[&to].send(msg).is_err() {
                warn!("send raft message to {} fail, let Raft retry it", to);
            }
        }
    }

    // Handle snapshot from leader.
    if !is_leader && !raft::is_empty_snap(ready.snapshot()) {
        // This is a snapshot, we need to apply the snapshot at first.
        raft_group.mut_store().wl().apply_snapshot(ready.snapshot().clone()).unwrap();
    }

    //////////////////////////////////////////////////////////////////////////
    // 2. Persist Raft Logs //////////////////////////////////////////////////

    // Persist raft logs.
    if let Err(e) = raft_group.raft.raft_log.store.wl().append(ready.entries()) {
        error!("persist raft log fail: {:?}, need to retry or panic", e);
        return;
    }

    //////////////////////////////////////////////////////////////////////////
    // 3. Persist HS Changes /////////////////////////////////////////////////

    // Persist HS changes if needed.
    if let Some(hs) = ready.hs() {
        raft_group.mut_store().wl().set_hardstate(hs.clone());
    }

    //////////////////////////////////////////////////////////////////////////
    // 4. ////////////////////////////////////////////////////////////////////

    // Proposed entries have now been committed. If this node is not a leader,
    // then it needs to respond to the leader that it has committed the entires.
    if !is_leader {
        for msg in ready.messages.drain(..) {
            let to = msg.get_to();
            if raft_group.raft.id == 2 {
                info!("NODE 2: Responding to message: {:?}", &msg);
            }
            if mailboxes[&to].send(msg).is_err() {
                warn!("send raft message to {} fail, let Raft retry it", to);
            }
        }
    }

    //////////////////////////////////////////////////////////////////////////
    // 5. Apply Committed Logs ///////////////////////////////////////////////
    //
    // Note: the storage interface here is only for committing the raft logs
    // not for application data (though the application data will also
    // appear in the raft log). We must take the committed raft logs and apply
    // them to our storage engine / state machine.

    // Apply all committed entries.
    if let Some(committed_entries) = ready.committed_entries.take() {
        for entry in committed_entries {
            if entry.get_data().is_empty() { continue; } // From new elected leaders.
            if let EntryType::EntryConfChange = entry.get_entry_type() {
                // For conf change messages, make them effective.
                let mut cc = ConfChange::new();
                cc.merge_from_bytes(entry.get_data()).unwrap();
                let node_id = cc.get_node_id();
                match cc.get_change_type() {
                    ConfChangeType::AddNode => raft_group.raft.add_node(node_id).unwrap(),
                    ConfChangeType::RemoveNode => raft_group.raft.remove_node(node_id).unwrap(),
                    ConfChangeType::AddLearnerNode => raft_group.raft.add_learner(node_id).unwrap(),
                    ConfChangeType::BeginMembershipChange
                    | ConfChangeType::FinalizeMembershipChange => unimplemented!(),
                }
            } else {
                // For normal proposals, extract the key-value pair and then
                // insert them into the kv engine.
                let data = str::from_utf8(entry.get_data()).unwrap();
                let reg = Regex::new("put ([0-9]+) (.+)").unwrap();
                if let Some(caps) = reg.captures(&data) {
                    kv_pairs.insert(caps[1].parse().unwrap(), caps[2].to_string());
                }
            }

            // If the node is the leader, respond to clients which submitted modifications.
            // This, of course, only applies to client initiated events.
            if is_leader {
                // NOTE: this is super crude and hardly correct.
                let proposal = proposals.lock().unwrap().pop_front().unwrap();
                proposal.propose_success.send(true).unwrap();
            }
        }
    }

    //////////////////////////////////////////////////////////////////////////
    // 6. Advance to Next State //////////////////////////////////////////////

    raft_group.advance(ready);
}

fn example_config() -> Config {
    Config {
        election_tick: 10,
        heartbeat_tick: 3,
        ..Default::default()
    }
}

// The message can be used to initialize a raft node or not.
fn is_initial_msg(msg: &Message) -> bool {
    let msg_type = msg.get_msg_type();
    msg_type == MessageType::MsgRequestVote
        || msg_type == MessageType::MsgRequestPreVote
        || (msg_type == MessageType::MsgHeartbeat && msg.get_commit() == 0)
}

struct Proposal {
    normal: Option<(u16, String)>, // key is an u16 integer, and value is a string.
    conf_change: Option<ConfChange>, // conf change.
    transfer_leader: Option<u64>,
    // If it's proposed, it will be set to the index of the entry.
    proposed: u64,
    propose_success: SyncSender<bool>,
}

impl Proposal {
    fn conf_change(cc: &ConfChange) -> (Self, Receiver<bool>) {
        let (tx, rx) = mpsc::sync_channel(1);
        let proposal = Proposal {
            normal: None,
            conf_change: Some(cc.clone()),
            transfer_leader: None,
            proposed: 0,
            propose_success: tx,
        };
        (proposal, rx)
    }

    fn normal(key: u16, value: String) -> (Self, Receiver<bool>) {
        let (tx, rx) = mpsc::sync_channel(1);
        let proposal = Proposal {
            normal: Some((key, value)),
            conf_change: None,
            transfer_leader: None,
            proposed: 0,
            propose_success: tx,
        };
        (proposal, rx)
    }
}

fn propose(raft_group: &mut RawNode<MemStorage>, proposal: &mut Proposal) {
    let last_index1 = raft_group.raft.raft_log.last_index() + 1;
    if let Some((ref key, ref value)) = proposal.normal {
        let data = format!("put {} {}", key, value).into_bytes();
        let _ = raft_group.propose(vec![], data);
    } else if let Some(ref cc) = proposal.conf_change {
        let _ = raft_group.propose_conf_change(vec![], cc.clone());
    } else if let Some(_tranferee) = proposal.transfer_leader {
        // TODO: implement tranfer leader.
        unimplemented!();
    }

    let last_index2 = raft_group.raft.raft_log.last_index() + 1;
    if last_index2 == last_index1 {
        // Propose failed, don't forget to respond to the client.
        proposal.propose_success.send(false).unwrap();
    } else {
        proposal.proposed = last_index1;
    }
}

// Proposes some conf change for peers [2, 5].
fn add_all_followers(proposals: &Mutex<VecDeque<Proposal>>) {
    for i in 2..6u64 {
        let mut conf_change = ConfChange::default();
        conf_change.set_node_id(i);
        conf_change.set_change_type(ConfChangeType::AddNode);
        loop {
            let (proposal, rx) = Proposal::conf_change(&conf_change);
            proposals.lock().unwrap().push_back(proposal);
            if rx.recv().unwrap() {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}
