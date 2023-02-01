use crossbeam_channel::{SendTimeoutError, Sender};
use massa_models::node::NodeId;
use massa_network_exports::{ConnectionId, NetworkError, NetworkEvent, NodeCommand, NodeEvent};
use std::thread::JoinHandle;
use std::time::Duration;
use tracing::debug;

pub struct EventSender {
    /// Sender for network events
    controller_event_tx: Sender<NetworkEvent>,
    /// Channel for sending node events.
    node_event_tx: Sender<NodeEvent>,
    /// Max time spend to wait
    max_send_wait: Duration,
}

impl EventSender {
    pub fn new(
        controller_event_tx: Sender<NetworkEvent>,
        node_event_tx: Sender<NodeEvent>,
        max_send_wait: Duration,
    ) -> Self {
        Self {
            controller_event_tx,
            node_event_tx,
            max_send_wait,
        }
    }

    pub fn send(&self, event: NetworkEvent) -> Result<(), NetworkError> {
        let result = self
            .controller_event_tx
            .send_timeout(event, self.max_send_wait);
        match result {
            Ok(()) => return Ok(()),
            Err(SendTimeoutError::Disconnected(event)) => {
                debug!(
                    "Failed to send NetworkEvent due to channel closure: {:?}.",
                    event
                );
            }
            Err(SendTimeoutError::Timeout(event)) => {
                debug!("Failed to send NetworkEvent due to timeout: {:?}.", event);
            }
        }
        Err(NetworkError::ChannelError("Failed to send event.".into()))
    }

    /// Forward a message to a node worker. If it fails, notify upstream about connection closure.
    pub fn forward(
        &self,
        node_id: NodeId,
        node: Option<&(ConnectionId, Sender<NodeCommand>, JoinHandle<()>)>,
        message: NodeCommand,
    ) {
        if let Some((_, node_command_tx, _)) = node {
            if node_command_tx.send(message).is_err() {
                debug!(
                    "{}",
                    NetworkError::ChannelError("contact with node worker lost while trying to send it a message. Probably a peer disconnect.".into())
                );
            };
        } else {
            // We probably weren't able to send this event previously,
            // retry it now.
            let _ = self.send(NetworkEvent::ConnectionClosed(node_id));
        }
    }

    pub fn clone_node_sender(&self) -> Sender<NodeEvent> {
        self.node_event_tx.clone()
    }
}

pub mod event_impl {
    use crate::network_worker::NetworkWorker;
    use massa_logging::massa_trace;
    use massa_models::{
        block_header::SecuredHeader,
        block_id::BlockId,
        endorsement::SecureShareEndorsement,
        node::NodeId,
        operation::{OperationPrefixIds, SecureShareOperation},
        secure_share::Id,
    };
    use massa_network_exports::{AskForBlocksInfo, BlockInfoReply, NodeCommand};
    use massa_network_exports::{NetworkError, NetworkEvent};
    use std::net::IpAddr;
    use tracing::{debug, info};
    macro_rules! evt_failed {
        ($err: ident) => {
            info!("Send network event failed {}", $err)
        };
    }

    // Implementation of the node event management functions
    pub fn on_received_peer_list(
        worker: &mut NetworkWorker,
        from: NodeId,
        list: &[IpAddr],
    ) -> Result<(), NetworkError> {
        debug!("node_id={} sent us a peer list ({} ips)", from, list.len());
        massa_trace!("peer_list_received", {
            "node_id": from,
            "ips": list
        });
        worker.peer_info_db.merge_candidate_peers(list)?;
        Ok(())
    }

    pub fn on_received_ask_for_blocks(
        worker: &mut NetworkWorker,
        from: NodeId,
        list: Vec<(BlockId, AskForBlocksInfo)>,
    ) {
        if let Err(err) = worker
            .event
            .send(NetworkEvent::AskedForBlocks { node: from, list })
        {
            evt_failed!(err)
        }
    }

    pub fn on_received_block_header(
        worker: &mut NetworkWorker,
        from: NodeId,
        header: SecuredHeader,
    ) -> Result<(), NetworkError> {
        massa_trace!(
            "network_worker.on_node_event receive NetworkEvent::ReceivedBlockHeader",
            {"hash": header.id.get_hash(), "header": header, "node": from}
        );
        if let Err(err) = worker.event.send(NetworkEvent::ReceivedBlockHeader {
            source_node_id: from,
            header,
        }) {
            evt_failed!(err)
        }
        Ok(())
    }

    pub fn on_received_block_info(
        worker: &mut NetworkWorker,
        from: NodeId,
        info: Vec<(BlockId, BlockInfoReply)>,
    ) -> Result<(), NetworkError> {
        if let Err(err) = worker
            .event
            .send(NetworkEvent::ReceivedBlockInfo { node: from, info })
        {
            evt_failed!(err)
        }
        Ok(())
    }

    pub fn on_asked_peer_list(
        worker: &mut NetworkWorker,
        from: NodeId,
    ) -> Result<(), NetworkError> {
        debug!("node_id={} asked us for peer list", from);
        massa_trace!("node_asked_peer_list", { "node_id": from });
        let peer_list = worker.peer_info_db.get_advertisable_peer_ips();
        if let Some((_, node_command_tx, _)) = worker.active_nodes.get(&from) {
            if node_command_tx
                .send(NodeCommand::SendPeerList(peer_list))
                .is_err()
            {
                debug!(
                    "{}",
                    NetworkError::ChannelError("node command send send_peer_list failed".into(),)
                );
            }
        } else {
            massa_trace!("node asked us for peer list and disappeared", {
                "node_id": from
            })
        }
        Ok(())
    }

    /// The node worker signal that he received some full `operations` from a
    /// node.
    ///
    /// Forward the event by sending a `[NetworkEvent::ReceivedOperations]`.
    /// See also `[massa_network_exports::NodeEventType::ReceivedOperations]`
    pub fn on_received_operations(
        worker: &mut NetworkWorker,
        from: NodeId,
        operations: Vec<SecureShareOperation>,
    ) {
        massa_trace!(
            "network_worker.on_node_event receive NetworkEvent::ReceivedOperations",
            { "operations": operations }
        );
        if let Err(err) = worker.event.send(NetworkEvent::ReceivedOperations {
            node: from,
            operations,
        }) {
            evt_failed!(err)
        }
    }

    /// The node worker signal that he received a batch of operation ids
    /// from another node.
    pub fn on_received_operations_annoncement(
        worker: &mut NetworkWorker,
        from: NodeId,
        operation_prefix_ids: OperationPrefixIds,
    ) {
        massa_trace!(
            "network_worker.on_node_event receive NetworkEvent::ReceivedOperationAnnouncements",
            { "operations": operation_prefix_ids }
        );
        if let Err(err) = worker
            .event
            .send(NetworkEvent::ReceivedOperationAnnouncements {
                node: from,
                operation_prefix_ids,
            })
        {
            evt_failed!(err)
        }
    }

    /// The node worker signal that he received a list of operations required
    /// from another node.
    pub fn on_received_ask_for_operations(
        worker: &mut NetworkWorker,
        from: NodeId,
        operation_prefix_ids: OperationPrefixIds,
    ) {
        massa_trace!(
            "network_worker.on_node_event receive NetworkEvent::ReceiveAskForOperations",
            { "operations": operation_prefix_ids }
        );
        if let Err(err) = worker.event.send(NetworkEvent::ReceiveAskForOperations {
            node: from,
            operation_prefix_ids,
        }) {
            evt_failed!(err)
        }
    }

    pub fn on_received_endorsements(
        worker: &mut NetworkWorker,
        from: NodeId,
        endorsements: Vec<SecureShareEndorsement>,
    ) {
        massa_trace!(
            "network_worker.on_node_event receive NetworkEvent::ReceivedEndorsements",
            { "endorsements": endorsements }
        );
        if let Err(err) = worker.event.send(NetworkEvent::ReceivedEndorsements {
            node: from,
            endorsements,
        }) {
            evt_failed!(err)
        }
    }
}
