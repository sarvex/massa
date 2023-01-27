// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::error::ModelsError;
use crate::prehash::PreHashed;
use massa_hash::{Hash, HashDeserializer};
use massa_serialization::{Deserializer, Serializer};
use massa_signature::PublicKey;
use nom::branch::alt;
use nom::character::complete::char;
use nom::error::{context, ContextError, ParseError};
use nom::sequence::preceded;
use nom::{IResult, Parser};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
mod sc_addr;
mod user_addr;
pub use sc_addr::*;
pub use user_addr::*;

/// Size of a serialized address, in bytes
pub const ADDRESS_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES + 1;

/// In future versions, the SC variant will encode slot, index and is_write directly
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Address {
    #[allow(missing_docs)]
    User(UserAddress),
    #[allow(missing_docs)]
    SC(HashedSCAddress),
}

impl std::fmt::Debug for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::User(arg0) => f.debug_tuple("Address").field(arg0).finish(),
            Self::SC(arg0) => f
                .debug_tuple("Address")
                .field(&SCAddress::from(arg0.clone()))
                .finish(),
        }
    }
}

const ADDRESS_PREFIX: char = 'A';

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}{}{}",
            ADDRESS_PREFIX,
            match self {
                Address::User(_) => 'U',
                Address::SC(_) => 'S',
            },
            match self {
                Address::User(usr) => usr.bs58_encode(),
                Address::SC(sc) => sc.bs58_encode(),
            }
            .map_err(|_| std::fmt::Error)?,
        )
    }
}

impl ::serde::Serialize for Address {
    fn serialize<S: ::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            s.collect_str(&self.to_string())
        } else {
            s.serialize_bytes(&self.prefixed_bytes())
        }
    }
}

impl<'de> ::serde::Deserialize<'de> for Address {
    fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<Address, D::Error> {
        if d.is_human_readable() {
            struct AddressVisitor;

            impl<'de> ::serde::de::Visitor<'de> for AddressVisitor {
                type Value = Address;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("A + {U | S} + base58::encode(version + hash)")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    if let Ok(v_str) = std::str::from_utf8(v) {
                        Address::from_str(v_str).map_err(E::custom)
                    } else {
                        Err(E::invalid_value(::serde::de::Unexpected::Bytes(v), &self))
                    }
                }

                fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    Address::from_str(v).map_err(E::custom)
                }
            }
            d.deserialize_str(AddressVisitor)
        } else {
            struct BytesVisitor;

            impl<'de> ::serde::de::Visitor<'de> for BytesVisitor {
                type Value = Address;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("a bytestring")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    Address::from_unprefixed_bytes(v).map_err(E::custom)
                }
            }

            d.deserialize_bytes(BytesVisitor)
        }
    }
}

impl FromStr for Address {
    type Err = ModelsError;
    /// ## Example
    /// ```rust
    /// # use massa_signature::{PublicKey, KeyPair, Signature};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// # use std::str::FromStr;
    /// # use massa_models::address::Address;
    /// # let keypair = KeyPair::generate();
    /// # let address = Address::from_public_key(&keypair.get_public_key());
    /// let ser = address.to_string();
    /// let res_addr = Address::from_str(&ser).unwrap();
    /// assert_eq!(address, res_addr);
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = Err(ModelsError::AddressParseError);
        let mut chars = s.chars();
        let Some('A') = chars.next() else {
            return err;
        };
        let Some(pref) = chars.next() else {
            return err;
        };

        let data = chars.collect::<String>();
        let res = match pref {
            'U' => Address::User(UserAddress::from_str(&data)?),
            'S' => Address::SC(HashedSCAddress::from_str(&data)?),
            _ => return err,
        };
        Ok(res)
    }
}

#[test]
fn test_address_str_format() {
    use massa_signature::KeyPair;

    let keypair = KeyPair::generate();
    let address = Address::from_public_key(&keypair.get_public_key());
    let a = address.to_string();
    let b = Address::from_str(&a).unwrap();
    assert_eq!(address, b);
}

impl PreHashed for Address {}

impl Address {
    /// Gets the associated thread. Depends on the `thread_count`
    pub fn get_thread(&self, thread_count: u8) -> u8 {
        match self {
            Address::User(usr) => usr.get_thread(thread_count),
            Address::SC(sc) => SCAddress::from(sc.clone()).thread(),
        }
    }

    /// If you know you have a UserAddress, you can get a direct reference, and avoind an alloc
    fn hash_bytes(&self) -> Vec<u8> {
        match self {
            Address::User(usr) => usr.0.to_bytes().to_vec(),
            Address::SC(sc) => sc.clone().into(),
        }
    }

    /// Computes address associated with given public key
    pub fn from_public_key(public_key: &PublicKey) -> Self {
        Address::User(UserAddress(Hash::compute_from(public_key.to_bytes())))
    }

    /// ## Example
    /// ```rust
    /// # use massa_signature::{PublicKey, KeyPair, Signature};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// # use massa_models::address::Address;
    /// # let keypair = KeyPair::generate();
    /// # let address = Address::from_public_key(&keypair.get_public_key());
    /// let bytes = address.prefixed_bytes();
    /// let res_addr = Address::from_prefixed_bytes(&bytes).unwrap();
    /// assert_eq!(address, res_addr);
    /// ```
    pub fn prefixed_bytes(&self) -> Vec<u8> {
        let pref = match self {
            Address::User(_) => b'U',
            Address::SC(_) => b'S',
        };
        [&[pref][..], &self.hash_bytes()].concat().to_vec()
    }

    // TODO: work out a scheme to determine if it's a User address or SC address?
    fn from_unprefixed_bytes(data: &[u8]) -> Result<Address, ModelsError> {
        Ok(Address::User(UserAddress(Hash::from_bytes(
            &data[0..32]
                .try_into()
                .map_err(|_| ModelsError::AddressParseError)?,
        ))))
    }
    /// ## Example
    /// ```rust
    /// # use massa_signature::{PublicKey, KeyPair, Signature};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// # use massa_models::address::Address;
    /// # let keypair = KeyPair::generate();
    /// # let address = Address::from_public_key(&keypair.get_public_key());
    /// let bytes = &address.prefixed_bytes();
    /// let res_addr = Address::from_prefixed_bytes(bytes).unwrap();
    /// assert_eq!(address, res_addr);
    /// ```
    pub fn from_prefixed_bytes(data: &[u8]) -> Result<Address, ModelsError> {
        let Some(pref) = data.first() else {
            return Err(ModelsError::AddressParseError);
        };

        match pref {
            b'U' => Ok(Address::User(UserAddress(Hash::from_bytes(
                &data[1..]
                    .try_into()
                    .map_err(|_| ModelsError::AddressParseError)?,
            )))),
            b'S' => Ok(Address::SC(data[1..].to_vec().into())),
            _ => Err(ModelsError::AddressParseError),
        }
    }
}

/// Serializer for `Address`
#[derive(Default, Clone)]
pub struct AddressSerializer;

impl AddressSerializer {
    /// Serializes an `Address` into a `Vec<u8>`
    pub fn new() -> Self {
        Self
    }
}

impl Serializer<Address> for AddressSerializer {
    fn serialize(
        &self,
        value: &Address,
        buffer: &mut Vec<u8>,
    ) -> Result<(), massa_serialization::SerializeError> {
        buffer.extend_from_slice(&value.prefixed_bytes());
        Ok(())
    }
}

/// Deserializer for `Address`
#[derive(Default, Clone)]
pub struct AddressDeserializer;

impl AddressDeserializer {
    /// Creates a new deserializer for `Address`
    pub const fn new() -> Self {
        Self
    }
}

impl Deserializer<Address> for AddressDeserializer {
    /// ## Example
    /// ```rust
    /// use massa_models::address::{Address, AddressDeserializer};
    /// use massa_serialization::{Deserializer, DeserializeError};
    /// use std::str::FromStr;
    ///
    /// let address = Address::from_str("AU12hgh5ULW9o8fJE9muLNXhQENaUUswQbxPyDSq8ridnDGu5gRiJ").unwrap();
    /// let bytes = address.prefixed_bytes();
    /// let (rest, res_addr) = AddressDeserializer::new().deserialize::<DeserializeError>(&bytes).unwrap();
    /// assert_eq!(address, res_addr);
    /// assert_eq!(rest.len(), 0);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Address, E> {
        context("Address Variant", alt((user_parser, sc_parser))).parse(buffer)
    }
}
// used to make the `alt(...)` more readable
fn user_parser<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
    input: &'a [u8],
) -> IResult<&'a [u8], Address, E> {
    context(
        "Failed after matching on 'U' Prefix",
        preceded(char('U'), |input| {
            HashDeserializer::new().deserialize(input)
        }),
    )
    .map(|hash| Address::User(UserAddress(hash)))
    .parse(input)
}
// used to make the `alt(...)` more readable
fn sc_parser<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
    input: &'a [u8],
) -> IResult<&'a [u8], Address, E> {
    context(
        "Failed after matching on 'S' Prefix",
        preceded(char('S'), |input| SCAddress::deserialize_bytes(input)),
    )
    .map(|inner| Address::SC(inner.into()))
    .parse(input)
}

/// Info for a given address on a given cycle
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionAddressCycleInfo {
    /// cycle number
    pub cycle: u64,
    /// true if that cycle is final
    pub is_final: bool,
    /// `ok_count` blocks were created by this address during that cycle
    pub ok_count: u64,
    /// `ok_count` blocks were missed by this address during that cycle
    pub nok_count: u64,
    /// number of active rolls the address had at that cycle (if still available)
    pub active_rolls: Option<u64>,
}
