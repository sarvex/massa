use crate::slot::Slot;

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SCAddress {
    slot: Slot,
    idx: u64,
    is_write: bool,
}

impl SCAddress {
    pub fn new(slot: Slot, idx: u64, is_write: bool) -> Self {
        Self {
            slot,
            idx,
            is_write,
        }
    }
}

struct SCAddressSerializer;
struct SCAddressDeerializer;
