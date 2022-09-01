use super::worker::{Worker, WorkerCommand, WorkerEvent};
use crate::node_info::NodeInfo;
use futures::stream::futures_unordered::FuturesUnordered;
use futures::StreamExt;
use massa_models::block::{BlockId, WrappedBlock, WrappedHeader};
use massa_models::node::NodeId;
use massa_models::operation::{OperationId, WrappedOperation};
use massa_models::prehash::{PreHashMap, PreHashSet};
use massa_network_exports::NetworkCommandSender;
use massa_protocol_exports::ProtocolEvent;
use massa_storage::Storage;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;

pub(crate) enum BlockRetrieverCommand {
    HeaderReceived {
        node_id: NodeId,
        header: WrappedHeader,
    },
    BlockOpListReceived {
        node_id: NodeId,
        block_id: BlockId,
        operation_ids: Vec<OperationId>,
    },
    BlockOpsReceived {
        node_id: NodeId,
        block_id: BlockId,
        storage: Storage,
    },
    WishlistDelta {
        added: PreHashMap<BlockId, Option<WrappedHeader>>,
        removed: PreHashSet<BlockId>,
    },
}

struct WorkerInfo {
    handle: JoinHandle<()>,
    worker_command_tx: mpsc::Sender<WorkerCommand>,
}

pub(crate) struct BlockRetriever {
    protocol_event_sender: mpsc::Sender<ProtocolEvent>,
    network_command_tx: NetworkCommandSender,
    command_rx: mpsc::Receiver<BlockRetrieverCommand>,
    worker_event_tx: mpsc::Sender<WorkerEvent>,
    worker_event_rx: mpsc::Receiver<WorkerEvent>,
    nodes: Arc<RwLock<HashMap<NodeId, NodeInfo>>>,
    workers: PreHashMap<BlockId, WorkerInfo>,
    storage: Storage
}

impl BlockRetriever {
    pub async fn launch(
        protocol_event_sender: mpsc::Sender<ProtocolEvent>,
        network_command_tx: NetworkCommandSender,
        nodes: Arc<RwLock<HashMap<NodeId, NodeInfo>>>,
        storage: &Storage
    ) -> (JoinHandle<()>, mpsc::Sender<BlockRetrieverCommand>) {
        let (command_tx, command_rx) = mpsc::channel(1024);
        let (worker_event_tx, worker_event_rx) = mpsc::channel(1024);
        let handle: JoinHandle<()> = tokio::spawn(async move {
            BlockRetriever {
                protocol_event_sender,
                network_command_tx,
                command_rx,
                worker_event_tx,
                worker_event_rx,
                nodes,
                workers: Default::default(),
                storage: storage.clone_without_refs()
            }
            .run()
            .await
        });
        (handle, command_tx)
    }

    async fn run(mut self) {
        loop {
            tokio::select! {
                // incoming commands
                opt_cmd = self.command_rx.recv() => match opt_cmd {
                    None => break,
                    Some(cmd) => self.process_command(cmd).await,
                },

                // worker events
                Some(evt) = self.worker_event_rx.recv() => self.process_worker_event(evt).await,
            }
        }

        // stop all workers
        self.workers
            .into_iter()
            .map(|(_, w_info)| w_info.handle)
            .collect::<FuturesUnordered<_>>()
            .collect::<Vec<_>>()
            .await;
    }

    async fn process_command(&mut self, cmd: BlockRetrieverCommand) {
        match cmd {
            BlockRetrieverCommand::WishlistDelta { added, removed } => {
                // stop `removed` workers
                removed
                    .into_iter()
                    .filter_map(|id| self.workers.remove(&id))
                    .map(|w_info| w_info.handle)
                    .collect::<FuturesUnordered<_>>()
                    .collect::<Vec<_>>()
                    .await;
                // launch `added` workers
                for (block_id, opt_header) in added {
                    self.start_worker(block_id, opt_header).await;
                }
            }

            block_cmd => { /* TODO DISPATCH DEPENDING ON NEEDS (block ID), ALSO HANDLE UNFILTERED BLOCK HEADER ANNOUNCEMENTS (send to ceonsensus) */
            }
        }
    }

    async fn process_worker_event(&mut self, evt: WorkerEvent) {
        // TODO report finished, request a node,
    }

    async fn start_worker(&mut self, block_id: BlockId, opt_header: Option<WrappedHeader>) {
        if self.workers.contains_key(&block_id) {
            // already running
            return;
        }
        let network_command_tx = self.network_command_tx.clone();
        let worker_event_tx = self.worker_event_tx.clone();
        let (worker_command_tx, worker_command_rx) = mpsc::channel(1024);
        let nodes = self.nodes.clone();
        let storage= self.storage.clone_without_refs();
        let handle = tokio::spawn(async move {
            Worker::new(
                network_command_tx,
                worker_command_rx,
                worker_event_tx,
                nodes,
                storage
            )
            .run(block_id, opt_header)
            .await
        });
        self.workers.insert(
            block_id,
            WorkerInfo {
                handle,
                worker_command_tx,
            },
        );
    }
}
