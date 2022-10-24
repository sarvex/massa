// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::sync::{
    mpsc::{self, Receiver},
    Arc, Mutex,
};

use massa_models::{
    api::BlockGraphStatus,
    block::{BlockHeader, BlockId},
    clique::Clique,
    slot::Slot,
    stats::ConsensusStats,
    wrapped::Wrapped,
};
use massa_storage::Storage;
use massa_time::MassaTime;

use crate::{
    block_graph_export::BlockGraphExport, bootstrapable_graph::BootstrapableGraph,
    error::GraphError, GraphController,
};

/// Test tool to mock graph controller responses
pub struct GraphEventReceiver(pub Receiver<MockGraphControllerMessage>);

/// List of possible messages you can receive from the mock
/// Each variant corresponds to a unique method in `GraphController`,
/// Some variants wait for a response on their `response_tx` field, if present.
/// See the documentation of `GraphController` for details on parameters and return values.
#[derive(Clone, Debug)]
pub enum MockGraphControllerMessage {
    GetBlockStatuses {
        block_ids: Vec<BlockId>,
        response_tx: mpsc::Sender<Vec<BlockGraphStatus>>,
    },
    GetBlockGraphStatuses {
        start_slot: Option<Slot>,
        end_slot: Option<Slot>,
        response_tx: mpsc::Sender<Result<BlockGraphExport, GraphError>>,
    },
    GetCliques {
        response_tx: mpsc::Sender<Vec<Clique>>,
    },
    GetBootstrapableGraph {
        response_tx: mpsc::Sender<Result<BootstrapableGraph, GraphError>>,
    },
    GetStats {
        response_tx: mpsc::Sender<Result<ConsensusStats, GraphError>>,
    },
    GetBestParents {
        response_tx: mpsc::Sender<Vec<(BlockId, u64)>>,
    },
    GetBlockcliqueBlockAtSlot {
        slot: Slot,
        response_tx: mpsc::Sender<Option<BlockId>>,
    },
    GetLatestBlockcliqueBlockAtSlot {
        slot: Slot,
        response_tx: mpsc::Sender<BlockId>,
    },
    MarkInvalidBlock {
        block_id: BlockId,
        header: Wrapped<BlockHeader, BlockId>,
    },
    RegisterBlock {
        block_id: BlockId,
        slot: Slot,
        block_storage: Storage,
    },
    RegisterBlockHeader {
        block_id: BlockId,
        header: Wrapped<BlockHeader, BlockId>,
    },
}

/// A mocked graph controller that will intercept calls on its methods
/// and emit corresponding `MockGraphControllerMessage` messages through a MPSC in a thread-safe way.
/// For messages with a `response_tx` field, the mock will await a response through their `response_tx` channel
/// in order to simulate returning this value at the end of the call.
#[derive(Clone)]
pub struct MockGraphController(Arc<Mutex<mpsc::Sender<MockGraphControllerMessage>>>);

impl MockGraphController {
    /// Create a new pair (mock graph controller, mpsc receiver for emitted messages)
    /// Note that unbounded mpsc channels are used
    pub fn new_with_receiver() -> (Box<dyn GraphController>, GraphEventReceiver) {
        let (tx, rx) = mpsc::channel();
        (
            Box::new(MockGraphController(Arc::new(Mutex::new(tx)))),
            GraphEventReceiver(rx),
        )
    }
}

impl GraphEventReceiver {
    /// wait command
    pub fn wait_command<F, T>(&mut self, timeout: MassaTime, filter_map: F) -> Option<T>
    where
        F: Fn(MockGraphControllerMessage) -> Option<T>,
    {
        match self.0.recv_timeout(timeout.into()) {
            Ok(msg) => filter_map(msg),
            Err(_) => None,
        }
    }
}

/// Implements all the methods of the `GraphController` trait,
/// but simply make them emit a `MockGraphControllerMessage`.
/// If the message contains a `response_tx`,
/// a response from that channel is read and returned as return value.
/// See the documentation of `GraphController` for details on each function.
impl GraphController for MockGraphController {
    fn get_block_graph_status(
        &self,
        start_slot: Option<Slot>,
        end_slot: Option<Slot>,
    ) -> Result<BlockGraphExport, GraphError> {
        let (response_tx, response_rx) = mpsc::channel();
        self.0
            .lock()
            .unwrap()
            .send(MockGraphControllerMessage::GetBlockGraphStatuses {
                start_slot,
                end_slot,
                response_tx,
            })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn get_block_statuses(&self, ids: &[BlockId]) -> Vec<BlockGraphStatus> {
        let (response_tx, response_rx) = mpsc::channel();
        self.0
            .lock()
            .unwrap()
            .send(MockGraphControllerMessage::GetBlockStatuses {
                block_ids: ids.to_vec(),
                response_tx,
            })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn get_cliques(&self) -> Vec<Clique> {
        let (response_tx, response_rx) = mpsc::channel();
        self.0
            .lock()
            .unwrap()
            .send(MockGraphControllerMessage::GetCliques { response_tx })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn get_bootstrap_graph(&self) -> Result<BootstrapableGraph, GraphError> {
        let (response_tx, response_rx) = mpsc::channel();
        self.0
            .lock()
            .unwrap()
            .send(MockGraphControllerMessage::GetBootstrapableGraph { response_tx })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn get_stats(&self) -> Result<ConsensusStats, GraphError> {
        let (response_tx, response_rx) = mpsc::channel();
        self.0
            .lock()
            .unwrap()
            .send(MockGraphControllerMessage::GetStats { response_tx })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn get_best_parents(&self) -> Vec<(BlockId, u64)> {
        let (response_tx, response_rx) = mpsc::channel();
        self.0
            .lock()
            .unwrap()
            .send(MockGraphControllerMessage::GetBestParents { response_tx })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn get_blockclique_block_at_slot(&self, slot: Slot) -> Option<BlockId> {
        let (response_tx, response_rx) = mpsc::channel();
        self.0
            .lock()
            .unwrap()
            .send(MockGraphControllerMessage::GetBlockcliqueBlockAtSlot { slot, response_tx })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn get_latest_blockclique_block_at_slot(&self, slot: Slot) -> BlockId {
        let (response_tx, response_rx) = mpsc::channel();
        self.0
            .lock()
            .unwrap()
            .send(MockGraphControllerMessage::GetLatestBlockcliqueBlockAtSlot { slot, response_tx })
            .unwrap();
        response_rx.recv().unwrap()
    }

    fn mark_invalid_block(&self, block_id: BlockId, header: Wrapped<BlockHeader, BlockId>) {
        self.0
            .lock()
            .unwrap()
            .send(MockGraphControllerMessage::MarkInvalidBlock { block_id, header })
            .unwrap();
    }

    fn register_block(&self, block_id: BlockId, slot: Slot, block_storage: Storage) {
        self.0
            .lock()
            .unwrap()
            .send(MockGraphControllerMessage::RegisterBlock {
                block_id,
                slot,
                block_storage,
            })
            .unwrap();
    }

    fn register_block_header(&self, block_id: BlockId, header: Wrapped<BlockHeader, BlockId>) {
        self.0
            .lock()
            .unwrap()
            .send(MockGraphControllerMessage::RegisterBlockHeader { block_id, header })
            .unwrap();
    }

    fn clone_box(&self) -> Box<dyn GraphController> {
        Box::new(self.clone())
    }
}