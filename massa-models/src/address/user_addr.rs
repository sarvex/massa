use std::ops::Bound::Included;
use std::str::FromStr;

use massa_hash::Hash;
use massa_serialization::{
    DeserializeError, Deserializer, SerializeError, Serializer, U64VarIntDeserializer,
    U64VarIntSerializer,
};

use crate::{config::THREAD_COUNT, error::ModelsError, prehash::PreHashed};
const USER_ADDRESS_VERSION: u64 = 0;
/// Derived from a public key.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UserAddress(pub Hash);
impl PreHashed for UserAddress {}

impl std::fmt::Debug for UserAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UserAddress")
            .field(
                &format!("thread(of {})", THREAD_COUNT),
                &self.get_thread(THREAD_COUNT),
            )
            .field("inner_addr", &self.bs58_encode())
            .finish()
    }
}

impl FromStr for UserAddress {
    type Err = ModelsError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let decoded_bs58_check = bs58::decode(s)
            .with_check(None)
            .into_vec()
            .map_err(|_| ModelsError::AddressParseError)?;
        let u64_deserializer = U64VarIntDeserializer::new(Included(0), Included(u64::MAX));
        let (rest, _version) = u64_deserializer
            .deserialize::<DeserializeError>(&decoded_bs58_check[..])
            .map_err(|_| ModelsError::AddressParseError)?;
        Ok(UserAddress(Hash::from_bytes(
            rest.try_into()
                .map_err(|_| ModelsError::AddressParseError)?,
        )))
    }
}

impl UserAddress {
    /// Gets the associated thread. Depends on the `thread_count`
    pub fn get_thread(&self, thread_count: u8) -> u8 {
        (self.0.to_bytes()[0])
            .checked_shr(8 - thread_count.trailing_zeros())
            .unwrap_or(0)
    }
    pub(crate) fn bs58_encode(&self) -> Result<String, SerializeError> {
        let u64_serializer = U64VarIntSerializer::new();
        // might want to allocate the vector with capacity in order to avoid re-allocation
        let mut bytes: Vec<u8> = Vec::new();
        u64_serializer.serialize(&USER_ADDRESS_VERSION, &mut bytes)?;
        bytes.extend(self.0.to_bytes());
        Ok(bs58::encode(bytes).with_check().into_string())
    }
}
