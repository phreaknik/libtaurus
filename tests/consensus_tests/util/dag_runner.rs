use chrono::{DateTime, Duration};
use cordelia::{
    avalanche, consensus::Dag, params, randomx::RandomXVMInstance, Block, GenesisConfig, Vertex,
    WireFormat,
};
use libp2p::{multihash::Multihash, PeerId};
use rand::{thread_rng, Rng};
use randomx_rs::RandomXFlag;
use std::{
    collections::{HashMap, HashSet},
    iter::once,
    ops::Deref,
    sync::Arc,
};
use tempfile::TempDir;
use tokio::sync::{broadcast, mpsc};

pub struct DagTestRunner<'a> {
    pub dag: Dag,
    pub randomx_vm: RandomXVMInstance,
    pub vertices: HashMap<&'a str, Vertex>,

    // Should vertices be inserted as slim vertices?
    slim_vertex_insertion_mode: bool,
}

impl<'a> DagTestRunner<'a> {
    /// Initialize the test runner for full-vertex insertion mode
    pub fn new_full_vertex_runner() -> DagTestRunner<'a> {
        let (dag, randomx_vm, vertices) = test_init();
        DagTestRunner {
            dag,
            randomx_vm,
            vertices,
            slim_vertex_insertion_mode: false,
        }
    }

    /// Initialize the test runner for slim-vertex insertion mode
    pub fn new_slim_vertex_runner() -> DagTestRunner<'a> {
        let (dag, randomx_vm, vertices) = test_init();
        DagTestRunner {
            dag,
            randomx_vm,
            vertices,
            slim_vertex_insertion_mode: true,
        }
    }

    /// Finds a nonce to satisfy the POW. Returns false if the block already had a valid nonce.
    pub fn mine_test_block(&self, block: &mut Block) -> bool {
        if block.verify_pow(&self.randomx_vm).is_ok() {
            false
        } else {
            block.nonce = thread_rng().gen();
            while block.verify_pow(&self.randomx_vm).is_err() {
                block.nonce += 1;
            }
            true
        }
    }

    /// Builds a block descending from the given parents
    pub fn build_test_block<V>(&self, parents: V, nonce: u64) -> Block
    where
        V: IntoIterator<Item = Arc<Vertex>>,
    {
        let (hashes, times): (Vec<_>, Vec<_>) = parents
            .into_iter()
            .map(|v| (v.hash(), v.block.as_ref().unwrap().time))
            .unzip();
        Block {
            version: 0,
            difficulty: params::MIN_DIFFICULTY,
            miner: PeerId::from_multihash(Multihash::default()).unwrap(),
            parents: hashes,
            inputs: Vec::new(),
            outputs: Vec::new(),
            time: *times.iter().max().unwrap() + Duration::seconds(1),
            nonce,
        }
    }

    /// Add a vertex to the list of test vertices
    pub fn add_test_vertex(&mut self, id: &'a str, vertex: Vertex) {
        self.vertices.insert(id, vertex);
    }

    /// Builds a list of vertices to create a DAG scenario.
    pub fn build_test_vertices<E>(&mut self, edges: E)
    where
        E: IntoIterator<Item = (&'a str, u64, Vec<&'a str>)>,
    {
        let mut bad_nonce = false;

        // Initialize a list of vertices with the frontier vertices
        for (vid, nonce, parents) in edges {
            let mut block = self.build_test_block(
                parents.iter().map(|vid| {
                    Arc::new(
                        self.vertices
                            .get(vid)
                            .expect(format!("didn't find {vid} in vertices").as_str())
                            .clone(),
                    )
                }),
                nonce,
            );
            let incorrect_start_nonce = self.mine_test_block(&mut block);
            if incorrect_start_nonce {
                println!("Block for {vid} should have had nonce {}", block.nonce);
            }
            bad_nonce |= incorrect_start_nonce;
            self.add_test_vertex(vid, Vertex::new_full(Arc::new(block)));
        }
        assert_eq!(bad_nonce, false, "Generated blocks have incorrect nonces");
    }

    /// Insert the specified vertices and return the result
    pub fn try_insert_vertices<I>(&mut self, ids: I) -> Result<(), avalanche::Error>
    where
        I: IntoIterator<Item = &'a str>,
    {
        let mut to_insert = Vec::new();
        for vertex in ids.into_iter().map(|vid| {
            self.vertices
                .get(vid)
                .expect("id does not exist: {vid}")
                .clone()
        }) {
            // If inserting slim vertices, need to register blocks first
            if self.slim_vertex_insertion_mode {
                let block = vertex.block.clone().unwrap();
                let vertex = Vertex::new_slim(block.hash(), vertex.parents);
                self.dag.register_block(block)?;
                to_insert.push(Arc::new(vertex));
            } else {
                to_insert.push(Arc::new(vertex));
            }
        }
        self.dag.try_insert_vertices(to_insert, None, false)
    }

    /// Check that the frontier contains EXACTLY the specified vertices, in any order
    pub fn frontier_matches<I>(&self, ids: I) -> bool
    where
        I: IntoIterator<Item = &'a str>,
    {
        let frontier: HashSet<_> = self.dag.get_frontier_hashes().into_iter().collect();
        let mut count = 0;
        for vid in ids {
            count += 1;
            if !frontier.contains(&self.vertices.get(vid).unwrap().hash()) {
                return false;
            }
        }
        count == frontier.len()
    }
}

fn test_init<'a>() -> (Dag, RandomXVMInstance, HashMap<&'a str, Vertex>) {
    let db_path = TempDir::new()
        .unwrap()
        .path()
        .join("test_outputs/consensus_db");
    let config = avalanche::Config {
        data_dir: db_path,
        genesis: GenesisConfig {
            difficulty: params::MIN_DIFFICULTY,
            time: DateTime::parse_from_rfc2822("Wed, 18 Feb 2015 23:16:09 GMT")
                .unwrap()
                .into(),
        }
        .to_vertex(),
        waitlist_cap: 10.try_into().unwrap(),
    };
    let randomx_vm =
        RandomXVMInstance::new(b"test-key", RandomXFlag::get_recommended_flags()).unwrap();
    let (action_sender, _action_receiver) = mpsc::unbounded_channel();
    let (event_sender, _) = broadcast::channel(10);

    // Construct a dag instance
    let mut dag = Dag::new(config, randomx_vm.clone(), action_sender, event_sender);

    // Initialize vertex list with the genesis vertex
    let vertices = once((
        "genesis",
        dag.get_vertex(&dag.genesis_hash())
            .unwrap()
            .0
            .deref()
            .clone(),
    ))
    .collect();

    (dag, randomx_vm, vertices)
}
