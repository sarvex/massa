use std::{collections::HashMap, sync::Arc};

use crate::node_info::NodeInfo;
use massa_models::{
    block::{BlockId, WrappedHeader, WrappedBlock, Block, BlockSerializer},
    node::NodeId, operation::OperationId, prehash::PreHashSet, wrapped::Wrapped,
};
use massa_network_exports::NetworkCommandSender;
use massa_storage::Storage;
use tokio::sync::{mpsc, RwLock};

pub(crate) enum WorkerCommand {}

pub(crate) enum WorkerEvent {}

pub(crate) struct Worker {
    network_command_tx: NetworkCommandSender,
    worker_command_rx: mpsc::Receiver<WorkerCommand>,
    worker_event_tx: mpsc::Sender<WorkerEvent>,
    nodes: Arc<RwLock<HashMap<NodeId, NodeInfo>>>,
    storage: Storage
}

impl Worker {
    pub(crate) fn new(
        network_command_tx: NetworkCommandSender,
        worker_command_rx: mpsc::Receiver<WorkerCommand>,
        worker_event_tx: mpsc::Sender<WorkerEvent>,
        nodes: Arc<RwLock<HashMap<NodeId, NodeInfo>>>,
        storage: Storage
    ) -> Self {
        Worker {
            network_command_tx,
            worker_command_rx,
            worker_event_tx,
            nodes,
            storage
        }
    }

    pub async fn run(
        mut self,
        block_id: BlockId,
        opt_header: Option<WrappedHeader>,
    ) -> Result<Storage, BlockRetrieverError> {
        // get header (checks header integrity)
        let header:WrappedHeader = match opt_header {
            Some(h) => h,
            None => self.retrieve_header(&block_id).await?
        };

        // prepare storage
        let mut block_storage = self.storage.clone_without_refs();
        block_storage.store_endorsements(header.content.endorsements.clone());

        // get block ops list (checks max number and global hash)
        let operation_ids: Vec<OperationId> = self.retrieve_block_op_list(&block_id, &header.content.operation_merkle_root).await?;

        // gather existing ops in storage
        let ops_set: PreHashSet<OperationId> = operation_ids.iter().copied().collect();
        let claimed_ops = block_storage.claim_operation_refs(&ops_set);
        let missing_ops = ops_set - claimed_ops;

        // if there are missing ops, query them
        if !missing_ops.is_empty() {
            self.retrieve_missing_ops(&mut block_storage, missing_ops).await?;
        }

        // build the block
        let wrapped_block: WrappedBlock = Wrapped {
            signature: header.signature,
            creator_public_key: header.creator_public_key,
            creator_address: header.creator_address,
            id: block_id,
            content: Block {
                header,
                operations: operation_ids
            },
            serialized_data: content_serialized,
        }
        .map_err(|err| BlockRetrieverError::Invalid(format!("could not build received block: {}", err)))?;

        Ok(block_storage)
    }
}
