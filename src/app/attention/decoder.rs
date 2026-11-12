//! Bounded, corruption-isolated attention metadata decoding.
use super::*;

// Lenient field decoding isolates corrupt/future attention from the rest of a
// Snapshot or storage. Unknown/nested payloads are consumed without retention.
#[derive(Default)]
enum Small {
    Text(String),
    Number(u64),
    Bool(bool),
    #[default]
    Invalid,
}
impl<'de> Deserialize<'de> for Small {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Small;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("bounded attention field")
            }
            fn visit_str<E: serde::de::Error>(self, s: &str) -> Result<Small, E> {
                Ok(if s.len() <= MAX_KEY_BYTES {
                    Small::Text(s.into())
                } else {
                    Small::Invalid
                })
            }
            fn visit_u64<E: serde::de::Error>(self, n: u64) -> Result<Small, E> {
                Ok(Small::Number(n))
            }
            fn visit_i64<E: serde::de::Error>(self, n: i64) -> Result<Small, E> {
                Ok(u64::try_from(n).map(Small::Number).unwrap_or_default())
            }
            fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Small, E> {
                Ok(Small::Invalid)
            }
            fn visit_bool<E: serde::de::Error>(self, b: bool) -> Result<Small, E> {
                Ok(Small::Bool(b))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Small, E> {
                Ok(Small::Invalid)
            }
            fn visit_none<E: serde::de::Error>(self) -> Result<Small, E> {
                Ok(Small::Invalid)
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut a: A) -> Result<Small, A::Error> {
                while a.next_element::<serde::de::IgnoredAny>()?.is_some() {}
                Ok(Small::Invalid)
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut a: A) -> Result<Small, A::Error> {
                while a
                    .next_entry::<serde::de::IgnoredAny, serde::de::IgnoredAny>()?
                    .is_some()
                {}
                Ok(Small::Invalid)
            }
        }
        d.deserialize_any(Visitor)
    }
}
impl Metadata {
    fn invalid() -> Self {
        Self {
            path: PathBuf::new(),
            cwd: PathBuf::new(),
            revision: String::new(),
            kind: Kind::Completed,
            at: 0,
            read: true,
        }
    }
}
impl<'de> Deserialize<'de> for Metadata {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "snake_case")]
        enum Field {
            Path,
            Cwd,
            Revision,
            Kind,
            At,
            Read,
            #[serde(other)]
            Unknown,
        }
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Metadata;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("attention metadata")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut a: A,
            ) -> Result<Metadata, A::Error> {
                let mut m = Metadata::invalid();
                let mut seen = 0u8;
                let mut invalid = false;
                while let Some(field) = a.next_key::<Field>()? {
                    if matches!(field, Field::Unknown) {
                        a.next_value::<serde::de::IgnoredAny>()?;
                        continue;
                    }
                    let bit = match field {
                        Field::Path => 1,
                        Field::Cwd => 2,
                        Field::Revision => 4,
                        Field::Kind => 8,
                        Field::At => 16,
                        Field::Read => 32,
                        Field::Unknown => unreachable!(),
                    };
                    invalid |= seen & bit != 0;
                    seen |= bit;
                    match (field, a.next_value::<Small>()?) {
                        (Field::Path, Small::Text(s)) if valid_path(Path::new(&s)) => {
                            m.path = s.into()
                        }
                        (Field::Cwd, Small::Text(s)) if valid_path(Path::new(&s)) => {
                            m.cwd = s.into()
                        }
                        (Field::Revision, Small::Text(s))
                            if s.len() == 36 && uuid::Uuid::parse_str(&s).is_ok() =>
                        {
                            m.revision = s
                        }
                        (Field::Kind, Small::Text(s)) => match s.as_str() {
                            "completed" => m.kind = Kind::Completed,
                            "needs_input" => m.kind = Kind::NeedsInput,
                            "failed" => m.kind = Kind::Failed,
                            _ => invalid = true,
                        },
                        (Field::At, Small::Number(n)) => m.at = n,
                        (Field::Read, Small::Bool(b)) => m.read = b,
                        _ => invalid = true,
                    }
                }
                Ok(if invalid || seen != 63 {
                    Metadata::invalid()
                } else {
                    m
                })
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Metadata, E> {
                Ok(Metadata::invalid())
            }
            fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Metadata, E> {
                Ok(Metadata::invalid())
            }
            fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<Metadata, E> {
                Ok(Metadata::invalid())
            }
            fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<Metadata, E> {
                Ok(Metadata::invalid())
            }
            fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<Metadata, E> {
                Ok(Metadata::invalid())
            }
            fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Metadata, E> {
                Ok(Metadata::invalid())
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut a: A,
            ) -> Result<Metadata, A::Error> {
                while a.next_element::<serde::de::IgnoredAny>()?.is_some() {}
                Ok(Metadata::invalid())
            }
        }
        d.deserialize_any(Visitor)
    }
}
#[derive(Default, Serialize, Deserialize)]
#[serde(transparent)]
pub(in crate::app) struct Stored(
    #[serde(deserialize_with = "deserialize_records")] pub Vec<Metadata>,
);

/// Discard excess metadata while decoding, not after retaining an unbounded Vec.
fn deserialize_records<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Metadata>, D::Error> {
    struct Bounded;
    impl<'de> serde::de::Visitor<'de> for Bounded {
        type Value = Vec<Metadata>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("bounded attention metadata")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> Result<Self::Value, A::Error> {
            let mut records = VecDeque::with_capacity(MAX_RECORDS);
            while let Some(record) = seq.next_element::<Metadata>()? {
                if !valid_metadata(&record, now()) {
                    continue;
                }
                if records.len() == MAX_RECORDS {
                    records.pop_front();
                }
                records.push_back(record);
            }
            Ok(records.into())
        }
    }
    deserializer.deserialize_seq(Bounded)
}
