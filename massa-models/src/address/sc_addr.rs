use core::fmt;
use std::ops::Bound::{Excluded, Included};
use std::str::FromStr;

use crate::prehash::PreHashed;
use crate::{
    config::THREAD_COUNT,
    error::ModelsError,
    slot::{Slot, SlotDeserializer, SlotSerializer},
};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::branch::alt;
use nom::bytes::complete::tag;
use nom::{error::context, IResult, Parser};

#[allow(missing_docs)]
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SCAddress {
    slot: Slot,
    idx: u64,
    is_write: bool,
}

#[allow(missing_docs)]
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct HashedSCAddress(Vec<u8>);

impl HashedSCAddress {
    /// Used for compact stringifying of the data.
    pub(crate) fn bs58_encode(&self) -> Result<String, SerializeError> {
        Ok(bs58::encode(&self.0).into_string())
    }
}
impl PreHashed for HashedSCAddress {}

impl From<HashedSCAddress> for SCAddress {
    fn from(value: HashedSCAddress) -> Self {
        todo!()
    }
}
impl From<HashedSCAddress> for Vec<u8> {
    fn from(value: HashedSCAddress) -> Self {
        // The PreHashed trait requires 8-byte len minimum
        if value.0.len() >= 8 {
            value.0
        } else {
            todo!("parse the bytes and return untruncated vals");
        }
    }
}
impl From<Vec<u8>> for HashedSCAddress {
    fn from(mut value: Vec<u8>) -> Self {
        // The PreHashed trait requires 8-byte len minimum
        if value.len() < 8 {
            value.resize(8, 0);
        }
        Self(value)
    }
}

impl From<SCAddress> for HashedSCAddress {
    fn from(value: SCAddress) -> Self {
        value
            .serialized_bytes()
            .expect("this should never fail")
            .into()
    }
}

impl std::fmt::Debug for SCAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SCAddress")
            .field("slot", &self.slot)
            .field("idx", &self.idx)
            .field("is_write", &self.is_write)
            .field(
                "encoded_address",
                &HashedSCAddress::from(*self)
                    .bs58_encode()
                    .map_err(|_| fmt::Error)?,
            )
            .finish()
    }
}

impl FromStr for HashedSCAddress {
    type Err = ModelsError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        bs58::decode(s)
            .into_vec()
            .map(Self)
            .map_err(|_| ModelsError::AddressParseError)
    }
}

impl SCAddress {
    #[allow(missing_docs)]
    pub fn thread(&self) -> u8 {
        self.slot.thread
    }
    #[allow(missing_docs)]
    pub fn new(slot: Slot, idx: u64, is_write: bool) -> Self {
        Self {
            slot,
            idx,
            is_write,
        }
    }

    /// Encodes the inner data of an address to its raw bytes.
    /// # Example
    /// ```rust
    /// use massa_models::address::SCAddress;
    /// use massa_models::slot::Slot;
    /// let addr = SCAddress::new(Slot::new(0, 1), 3, true);
    /// assert_eq!(addr.serialized_bytes().unwrap(), [0,1,3,1]);
    /// ```
    pub fn serialized_bytes(&self) -> Result<Vec<u8>, SerializeError> {
        let mut res = vec![];
        SCAddressSerializer.serialize(self, &mut res)?;
        Ok(res)
    }
    /// Encodes the inner data of an address to its raw bytes. The inverse of `serialized_bytes`
    /// # Example
    /// ```rust
    /// use massa_models::address::SCAddress;
    /// use massa_models::slot::Slot;
    /// use massa_serialization::DeserializeError;
    /// let addr = SCAddress::new(Slot::new(0, 1), 3, true);
    /// assert_eq!(SCAddress::deserialize_bytes::<DeserializeError>(&[0,1,3,1]).unwrap(), ([].as_slice(), addr));
    /// ```
    pub fn deserialize_bytes<
        'a,
        E: nom::error::ParseError<&'a [u8]> + nom::error::ContextError<&'a [u8]>,
    >(
        bytes: &'a [u8],
    ) -> IResult<&'a [u8], Self, E> {
        SCAddressDeserializer.deserialize::<E>(&bytes)
    }
}

struct SCAddressSerializer;
struct SCAddressDeserializer;

impl Serializer<SCAddress> for SCAddressSerializer {
    fn serialize(&self, value: &SCAddress, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        SlotSerializer::new().serialize(&value.slot, buffer)?;
        U64VarIntSerializer::new().serialize(&value.idx, buffer)?;
        if value.is_write {
            buffer.push(1);
        } else {
            buffer.push(0);
        }
        Ok(())
    }
}

impl Deserializer<SCAddress> for SCAddressDeserializer {
    fn deserialize<'a, E: nom::error::ParseError<&'a [u8]> + nom::error::ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> nom::IResult<&'a [u8], SCAddress, E> {
        let (rest, slot) = context("Invalid slot", |input| {
            SlotDeserializer::new(
                (Included(0), Excluded(u64::MAX)),
                (Included(0), Excluded(THREAD_COUNT)),
            )
            .deserialize(input)
        })
        .parse(buffer)?;
        let u64deser = U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX));
        let (rest, idx) =
            context("Invalid index", |input| u64deser.deserialize(input)).parse(rest)?;

        let (rest, is_write) =
            context("Is Write byte", |input| alt((tag([0]), tag([1])))(input)).parse(rest)?;

        Ok((
            rest,
            SCAddress {
                slot,
                idx,
                is_write: is_write[0] == 1,
            },
        ))
    }
}

#[cfg(test)]
mod test {
    use crate::address::Address;

    use super::*;

    #[test]
    fn serde_loop() {
        let addr = SCAddress::new(Slot::new(0, 1), 3, true);
        let bytes = addr.serialized_bytes().unwrap();
        let res = SCAddress::deserialize_bytes::<DeserializeError>(&bytes)
            .unwrap()
            .1;
        assert_eq!(addr, res);
    }

    #[test]
    fn str_loop() {
        let raw = "1LiC";
        let addr = SCAddress::new(Slot::new(0, 1), 3, true);

        // intermediarry check that the un-prefixed string is as we want.
        assert_eq!(addr.bs58_encode().unwrap().to_string(), raw);

        // add the leaders
        let with_leaders = format!("AS{}", raw);
        let wrapped_addr = Address::SC(addr);

        // check the wrapped form generates the leaders
        assert_eq!(format!("{}", wrapped_addr), with_leaders);

        assert_eq!(wrapped_addr, Address::from_str(&with_leaders).unwrap());
        assert_eq!(addr, SCAddress::from_str(&raw).unwrap())
    }
}
