use massa_hash::Hash;
use massa_serialization::{Serializer, U64VarIntSerializer};
const USER_ADDRESS_VERSION: u64 = 0;
/// Derived from a public key.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UserAddress(pub Hash);

impl std::fmt::Display for UserAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let u64_serializer = U64VarIntSerializer::new();
        // might want to allocate the vector with capacity in order to avoid re-allocation
        let mut bytes: Vec<u8> = Vec::new();
        u64_serializer
            .serialize(&USER_ADDRESS_VERSION, &mut bytes)
            .map_err(|_| std::fmt::Error)?;
        bytes.extend(self.0.to_bytes());
        write!(f, "{}", bs58::encode(bytes).with_check().into_string())
    }
}
