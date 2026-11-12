//! Streaming, forgiving checkpoint ingestion: unknown/overflow values are discarded,
//! never collected as a generic JSON tree. Only bounded labels and root strings survive.
use super::*;
use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};

#[derive(Deserialize)]
#[serde(field_identifier, rename_all = "snake_case")]
enum Field {
    Projects,
    Root,
    Labels,
    Selected,
    #[serde(other)]
    Other,
}
use std::{fmt, marker::PhantomData};

trait Shape: Default {
    fn number(_: u64) -> Self {
        Self::default()
    }
    fn string(_: &str) -> Self {
        Self::default()
    }
    fn map<'de, A: MapAccess<'de>>(mut map: A) -> Result<Self, A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(Self::default())
    }
    fn seq<'de, A: SeqAccess<'de>>(mut seq: A) -> Result<Self, A::Error> {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(Self::default())
    }
}
struct Decode<T>(PhantomData<T>);
impl<T> Default for Decode<T> {
    fn default() -> Self {
        Self(PhantomData)
    }
}
impl<'de, T: Shape> DeserializeSeed<'de> for Decode<T> {
    type Value = T;
    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<T, D::Error> {
        deserializer.deserialize_any(self)
    }
}
impl<'de, T: Shape> Visitor<'de> for Decode<T> {
    type Value = T;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("bounded terminal presentation metadata")
    }
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<T, A::Error> {
        T::map(map)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<T, A::Error> {
        T::seq(seq)
    }
    fn visit_u64<E: serde::de::Error>(self, n: u64) -> Result<T, E> {
        Ok(T::number(n))
    }
    fn visit_i64<E: serde::de::Error>(self, n: i64) -> Result<T, E> {
        Ok(u64::try_from(n).map(T::number).unwrap_or_default())
    }
    fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<T, E> {
        Ok(T::default())
    }
    fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<T, E> {
        Ok(T::default())
    }
    fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<T, E> {
        Ok(T::string(text))
    }
    fn visit_string<E: serde::de::Error>(self, text: String) -> Result<T, E> {
        Ok(T::string(&text))
    }
    fn visit_none<E: serde::de::Error>(self) -> Result<T, E> {
        Ok(T::default())
    }
    fn visit_unit<E: serde::de::Error>(self) -> Result<T, E> {
        Ok(T::default())
    }
    fn visit_some<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<T, D::Error> {
        DeserializeSeed::deserialize(self, deserializer)
    }
}
#[derive(Default)]
struct Number(Option<u64>);
impl Shape for Number {
    fn number(n: u64) -> Self {
        Self(Some(n))
    }
}
#[derive(Default)]
struct Root(PathBuf);
impl Shape for Root {
    fn string(text: &str) -> Self {
        if valid_root(Path::new(text)) {
            Self(text.into())
        } else {
            Self::default()
        }
    }
}
struct List<T, const N: usize>(Vec<T>);
impl<T, const N: usize> Default for List<T, N> {
    fn default() -> Self {
        Self(Vec::new())
    }
}
impl<T: Shape, const N: usize> Shape for List<T, N> {
    fn seq<'de, A: SeqAccess<'de>>(mut seq: A) -> Result<Self, A::Error> {
        let mut items = Vec::new();
        for _ in 0..N {
            let Some(item) = seq.next_element_seed(Decode::<T>::default())? else {
                return Ok(Self(items));
            };
            items.push(item);
        }
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(Self(items))
    }
}
impl Shape for SavedProject {
    fn map<'de, A: MapAccess<'de>>(mut map: A) -> Result<Self, A::Error> {
        let mut project = Self::default();
        while let Some(key) = map.next_key::<Field>()? {
            match key {
                Field::Root => project.root = map.next_value_seed(Decode::<Root>::default())?.0,
                Field::Labels => {
                    project.labels = map
                        .next_value_seed(Decode::<List<Number, { tabs::MAX_TABS }>>::default())?
                        .0
                        .into_iter()
                        .map(|number| number.0.unwrap_or(0))
                        .collect()
                }
                Field::Selected => {
                    project.selected = map.next_value_seed(Decode::<Number>::default())?.0
                }
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(project)
    }
}
impl Shape for SavedTabs {
    fn map<'de, A: MapAccess<'de>>(mut map: A) -> Result<Self, A::Error> {
        let mut saved = Self::default();
        while let Some(key) = map.next_key::<Field>()? {
            if matches!(key, Field::Projects) {
                saved.projects = map
                    .next_value_seed(Decode::<List<SavedProject, MAX_ROOTS>>::default())?
                    .0;
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        saved.bound();
        Ok(saved)
    }
}
impl<'de> Deserialize<'de> for SavedTabs {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Decode::<Self>::default().deserialize(deserializer)
    }
}
