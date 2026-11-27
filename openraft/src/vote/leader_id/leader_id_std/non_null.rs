use std::fmt::Display;

use serde::Serialize;
use serde::Serializer;
use serde::ser::Error;

/// Check the top-level representation without buffering or serializing the node ID twice.
pub(super) struct NonNull<'a, T>(pub(super) &'a T)
where T: ?Sized;

impl<T> Serialize for NonNull<'_, T>
where T: Serialize + ?Sized
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        self.0.serialize(RejectNull(serializer))
    }
}

struct RejectNull<S>(S);

macro_rules! forward {
    ($($method:ident ($($arg:ident: $ty:ty),*) -> $ok:ident;)*) => {
        $(
            fn $method(self, $($arg: $ty),*) -> Result<Self::$ok, Self::Error> {
                self.0.$method($($arg),*)
            }
        )*
    };
}

impl<S> Serializer for RejectNull<S>
where S: Serializer
{
    type Ok = S::Ok;
    type Error = S::Error;
    type SerializeSeq = S::SerializeSeq;
    type SerializeTuple = S::SerializeTuple;
    type SerializeTupleStruct = S::SerializeTupleStruct;
    type SerializeTupleVariant = S::SerializeTupleVariant;
    type SerializeMap = S::SerializeMap;
    type SerializeStruct = S::SerializeStruct;
    type SerializeStructVariant = S::SerializeStructVariant;

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        Err(Error::custom("LeaderId.voted_for must not serialize as null"))
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        self.serialize_none()
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        self.serialize_none()
    }

    fn serialize_f32(self, value: f32) -> Result<Self::Ok, Self::Error> {
        if value.is_finite() {
            self.0.serialize_f32(value)
        } else {
            self.serialize_none()
        }
    }

    fn serialize_f64(self, value: f64) -> Result<Self::Ok, Self::Error> {
        if value.is_finite() {
            self.0.serialize_f64(value)
        } else {
            self.serialize_none()
        }
    }

    // These wrappers may be transparent, so check their payload too. Leave compound values and
    // tagged variants alone: a null inside one is not itself a null node ID.
    fn serialize_some<T>(self, value: &T) -> Result<Self::Ok, Self::Error>
    where T: Serialize + ?Sized {
        self.0.serialize_some(&NonNull(value))
    }

    fn serialize_newtype_struct<T>(self, name: &'static str, value: &T) -> Result<Self::Ok, Self::Error>
    where T: Serialize + ?Sized {
        self.0.serialize_newtype_struct(name, &NonNull(value))
    }

    fn serialize_newtype_variant<T>(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: Serialize + ?Sized,
    {
        self.0.serialize_newtype_variant(name, index, variant, value)
    }

    fn collect_str<T>(self, value: &T) -> Result<Self::Ok, Self::Error>
    where T: Display + ?Sized {
        self.0.collect_str(value)
    }

    fn collect_seq<I>(self, iter: I) -> Result<Self::Ok, Self::Error>
    where
        I: IntoIterator,
        I::Item: Serialize,
    {
        self.0.collect_seq(iter)
    }

    fn collect_map<K, V, I>(self, iter: I) -> Result<Self::Ok, Self::Error>
    where
        K: Serialize,
        V: Serialize,
        I: IntoIterator<Item = (K, V)>,
    {
        self.0.collect_map(iter)
    }

    fn is_human_readable(&self) -> bool {
        self.0.is_human_readable()
    }

    forward! {
        serialize_bool(value: bool) -> Ok;
        serialize_i8(value: i8) -> Ok;
        serialize_i16(value: i16) -> Ok;
        serialize_i32(value: i32) -> Ok;
        serialize_i64(value: i64) -> Ok;
        serialize_i128(value: i128) -> Ok;
        serialize_u8(value: u8) -> Ok;
        serialize_u16(value: u16) -> Ok;
        serialize_u32(value: u32) -> Ok;
        serialize_u64(value: u64) -> Ok;
        serialize_u128(value: u128) -> Ok;
        serialize_char(value: char) -> Ok;
        serialize_str(value: &str) -> Ok;
        serialize_bytes(value: &[u8]) -> Ok;
        serialize_unit_variant(name: &'static str, index: u32, variant: &'static str) -> Ok;
        serialize_seq(len: Option<usize>) -> SerializeSeq;
        serialize_tuple(len: usize) -> SerializeTuple;
        serialize_tuple_struct(name: &'static str, len: usize) -> SerializeTupleStruct;
        serialize_tuple_variant(name: &'static str, index: u32, variant: &'static str, len: usize) -> SerializeTupleVariant;
        serialize_map(len: Option<usize>) -> SerializeMap;
        serialize_struct(name: &'static str, len: usize) -> SerializeStruct;
        serialize_struct_variant(name: &'static str, index: u32, variant: &'static str, len: usize) -> SerializeStructVariant;
    }
}

#[cfg(test)]
mod tests {
    use derive_more::Display;
    use serde::Deserialize;
    use serde::Serialize;

    use super::NonNull;
    use crate::NodeId;
    use crate::impls::leader_id_std::LeaderId;
    use crate::vote::RaftLeaderId;

    #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize, Display)]
    #[display("only")]
    struct OnlyNode;

    #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize, Display)]
    #[serde(untagged)]
    enum UntaggedId {
        #[display("only")]
        Only(OnlyNode),
        #[display("{name}")]
        Named { name: String },
    }

    #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize, Display)]
    #[display("wrapped")]
    struct WrappedId(Option<Option<u64>>);

    #[test]
    fn test_leader_id_serde_reject_null_node_id() {
        fn assert_null_rejected<NID>(id: NID)
        where NID: NodeId {
            let err = serde_json::to_string(&LeaderId::new(5_u64, id)).unwrap_err();
            assert_eq!("LeaderId.voted_for must not serialize as null", err.to_string());
        }

        tracing::info!(
            term = 5,
            "reject IDs whose JSON encoding is indistinguishable from no vote"
        );
        {
            assert_null_rejected(OnlyNode);
            assert_null_rejected(UntaggedId::Only(OnlyNode));
            assert_null_rejected(WrappedId(Some(None)));
        }

        tracing::info!(
            term = 5,
            "do not reinterpret a historical null vote as the unit node ID"
        );
        {
            let legacy = r#"{"term":5,"voted_for":null}"#;
            let err = serde_json::from_str::<LeaderId<u64, OnlyNode>>(legacy).unwrap_err();
            assert!(err.to_string().starts_with("LeaderId.voted_for must not be null"));
        }
    }

    #[test]
    fn test_leader_id_serde_reject_non_finite_id_encoding() {
        tracing::info!("reject non-finite floats that JSON would turn into a null candidate");
        {
            let err = serde_json::to_string(&NonNull(&f32::INFINITY)).unwrap_err();
            assert_eq!("LeaderId.voted_for must not serialize as null", err.to_string());
            let err = serde_json::to_string(&NonNull(&f64::NAN)).unwrap_err();
            assert_eq!("LeaderId.voted_for must not serialize as null", err.to_string());
        }
    }

    #[test]
    fn test_leader_id_serde_round_trips_tagged_node_id() -> anyhow::Result<()> {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize, Display)]
        #[serde(rename_all = "snake_case")]
        enum TaggedId {
            #[display("only")]
            Only(OnlyNode),
        }

        tracing::info!(
            term = 5,
            "round-trip a tagged ID whose null payload does not hide the candidate"
        );
        {
            let lid = LeaderId::new(5_u64, TaggedId::Only(OnlyNode));
            let json = serde_json::to_string(&lid)?;
            assert_eq!(r#"{"term":5,"voted_for":{"only":null}}"#, json);
            let decoded: LeaderId<u64, TaggedId> = serde_json::from_str(&json)?;
            assert_eq!(lid, decoded);

            let binary = bincode::serialize(&lid)?;
            let decoded: LeaderId<u64, TaggedId> = bincode::deserialize(&binary)?;
            assert_eq!(lid, decoded);
        }

        Ok(())
    }
}
