// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_factory_exports::{FactoryChannels, FactoryConfig};
use massa_models::{
    timeslots::{get_block_slot_timestamp, get_closest_slot_to_timestamp},
    wrapped::WrappedContent,
    BlockId, Endorsement, EndorsementSerializer, Slot, WrappedEndorsement,
};
use massa_signature::KeyPair;
use massa_time::MassaTime;
use massa_wallet::Wallet;
use std::{
    sync::{mpsc, Arc, RwLock},
    thread,
    time::Instant,
};
use tracing::{debug, warn};

/// Structure gathering all elements needed by the factory thread
pub(crate) struct EndorsementFactoryWorker {
    cfg: FactoryConfig,
    wallet: Arc<RwLock<Wallet>>,
    channels: FactoryChannels,
    factory_receiver: mpsc::Receiver<()>,
    endorsement_serializer: EndorsementSerializer,
    half_t0: MassaTime,
}

impl EndorsementFactoryWorker {
    /// Creates the `FactoryThread` structure to gather all data and references
    /// needed by the factory worker thread.
    pub(crate) fn spawn(
        cfg: FactoryConfig,
        wallet: Arc<RwLock<Wallet>>,
        channels: FactoryChannels,
        factory_receiver: mpsc::Receiver<()>,
    ) -> thread::JoinHandle<()> {
        thread::Builder::new()
            .name("endorsement factory worker".into())
            .spawn(|| {
                let mut this = Self {
                    half_t0: cfg
                        .t0
                        .checked_div_u64(2)
                        .expect("could not compute half_t0"),
                    cfg,
                    wallet,
                    channels,
                    factory_receiver,
                    endorsement_serializer: EndorsementSerializer::new(),
                };
                this.run();
            })
            .expect("could not spawn endorsement factory worker thread")
    }

    /// Gets the next slot and the instant when the corresponding endorsements should be made.
    /// Slots can be skipped if we waited too much in-between.
    /// Extra safety against double-production caused by clock adjustments (this is the role of the previous_slot parameter).
    fn get_next_slot(&self, previous_slot: Option<Slot>) -> (Slot, Instant) {
        // get current absolute time
        let now = MassaTime::compensated_now(self.cfg.clock_compensation_millis)
            .expect("could not get current time");

        // if it's the first computed slot, add a time shift to prevent double-production on node restart with clock skew
        let base_time = if previous_slot.is_none() {
            now.saturating_add(self.cfg.initial_delay)
        } else {
            now
        };

        // get closest slot according to the current absolute time
        let mut next_slot = get_closest_slot_to_timestamp(
            self.cfg.thread_count,
            self.cfg.t0,
            self.cfg.genesis_timestamp,
            base_time,
        );

        // protection against double-production on unexpected system clock adjustment
        if let Some(prev_slot) = previous_slot {
            if next_slot <= prev_slot {
                next_slot = prev_slot
                    .get_next_slot(self.cfg.thread_count)
                    .expect("could not compute next slot");
            }
        }

        // get the tiemstamp of the target slot
        let next_instant = get_block_slot_timestamp(
            self.cfg.thread_count,
            self.cfg.t0,
            self.cfg.genesis_timestamp,
            next_slot,
        )
        .expect("could not get block slot timestamp")
        .saturating_add(self.half_t0)
        .estimate_instant(self.cfg.clock_compensation_millis)
        .expect("could not estimate block slot instant");

        (next_slot, next_instant)
    }

    /// Interruptibly wait until an instant or a stop signal
    ///
    /// # Return value
    /// Returns `true` if the instant was reached, otherwise `false` if there was an interruption.
    fn interruptible_wait_until(&self, duration: Instant) -> bool {
        match self.factory_receiver.recv_deadline(duration) {
            // message received => quit main loop
            Ok(()) => false,
            // timeout => continue main loop
            Err(mpsc::RecvTimeoutError::Timeout) => true,
            // channel disconnected (sender dropped) => quit main loop
            Err(mpsc::RecvTimeoutError::Disconnected) => false,
        }
    }

    /// Process a slot: produce a block at that slot if one of the managed keys is drawn.
    fn process_slot(&mut self, slot: Slot) {
        // get endorsement producer addresses for that slot
        let producer_addrs = match self.channels.selector.get_selection(slot) {
            Ok(sel) => sel.endorsements,
            Err(err) => {
                warn!(
                    "endorsement factory could not get selector draws for slot {}: {}",
                    slot, err
                );
                return;
            }
        };

        // get creators if they are managed by our wallet
        let producers_indices: Vec<(KeyPair, usize)> = Vec::new();
        {
            let wallet = self.wallet.read().expect("could not lock wallet");
            for (index, producer_addr) in producer_addrs.into_iter().enumerate() {
                // check if the block producer address is handled by the wallet
                let producer_keypair = match wallet.find_associated_keypair(&producer_addr) {
                    // the selected block producer is managed locally => continue to attempt endorsement production
                    Some(kp) => (kp, index),
                    // the selected block producer is not managed locally => continue
                    None => continue,
                };
            }
        }

        // quit if there is nothing to produce
        if producers_indices.is_empty() {
            return;
        }

        // get consensus block ID for that slot
        let endorsed_block: BlockId =
            match self.channels.consensus.get_blockclique_block_at_slot(slot) {
                // error getting block ID at target slot
                Err(_) => {
                    warn!(
                        "could not get blockclique block to create endorsement targeting slot {}",
                        slot
                    );
                    return;
                }

                // the target slot is a miss: ignore
                Ok(None) => return,

                // there is a block a the target slot
                Ok(Some(b_id)) => b_id,
            };

        // produce endorsements
        let mut endorsements: Vec<WrappedEndorsement> = Vec::with_capacity(producers_indices.len());
        for (keypair, index) in producers_indices {
            let endorsement = Endorsement::new_wrapped(
                Endorsement {
                    slot,
                    index: index as u32,
                    endorsed_block,
                },
                EndorsementSerializer::new(),
                &keypair,
            )
            .expect("could not create endorsement");

            // log endorsement creation
            debug!(
                "endorsement {} created at slot {} by address {}",
                endorsement.id, endorsement.content.slot, endorsement.creator_address
            );

            endorsements.push(endorsement);
        }

        // store endorsements
        let mut endo_storage = self.channels.storage.clone_without_refs();
        endo_storage.store_endorsements(endorsements);

        // send endorsement to pool for listing and propagation
        self.channels.pool.add_endorsements(endo_storage);
    }

    /// main run loop of the endorsement creator thread
    fn run(&mut self) {
        let mut prev_slot = None;
        loop {
            // get next slot
            let (slot, endorsement_instant) = self.get_next_slot(prev_slot);

            // wait until slot
            if !self.interruptible_wait_until(endorsement_instant) {
                break;
            }

            // process slot
            self.process_slot(slot);

            // update previous slot
            prev_slot = Some(slot);
        }
    }
}