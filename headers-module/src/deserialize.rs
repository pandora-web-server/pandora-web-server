// Copyright 2024 Wladimir Palant
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Custom deserialization code for the configuration

use http::header::{HeaderName, HeaderValue};
use module_utils::{DeserializeMap, MapVisitor};
use serde::{
    de::{DeserializeSeed, Deserializer, Error as _, MapAccess, SeqAccess, Visitor},
    Deserialize,
};
use std::collections::HashMap;

use crate::configuration::{MatchRule, MatchRules, WithMatchRules};

/// Normalizes a path by removing leading and trailing slashes as well as collapsing multiple
/// separating slashes into one.
fn normalize_path(path: &str) -> String {
    let mut had_slash = true;
    let mut path: String = path
        .chars()
        .filter(|c| {
            if *c == '/' {
                if had_slash {
                    false
                } else {
                    had_slash = true;
                    true
                }
            } else {
                had_slash = false;
                true
            }
        })
        .collect();

    if path.ends_with('/') {
        path.pop();
    }

    path
}

impl<T: AsRef<str>> From<T> for MatchRule {
    fn from(value: T) -> Self {
        let mut rule = value.as_ref().trim();
        let prefix = if let Some(r) = rule.strip_suffix("/*") {
            rule = r;
            true
        } else {
            !rule.contains('/')
        };

        let (host, path) = if rule.starts_with('/') {
            ("", rule)
        } else if let Some((host, path)) = rule.split_once('/') {
            (host, path)
        } else {
            (rule, "")
        };

        Self {
            host: host.to_owned(),
            path: normalize_path(path),
            prefix,
        }
    }
}

impl<'de> Deserialize<'de> for MatchRule {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(String::deserialize(deserializer)?.into())
    }
}

pub(crate) fn deserialize_match_rule_list<'de, D>(
    seed: Vec<MatchRule>,
    deserializer: D,
) -> Result<Vec<MatchRule>, D::Error>
where
    D: Deserializer<'de>,
{
    struct ListVisitor {
        seed: Vec<MatchRule>,
    }
    impl<'de> Visitor<'de> for ListVisitor {
        type Value = Vec<MatchRule>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("String or Vec<String>")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut list = self.seed;
            while let Some(entry) = seq.next_element()? {
                list.push(entry);
            }
            Ok(list)
        }

        fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            let mut list = self.seed;
            list.push(MatchRule::from(v));
            Ok(list)
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            let mut list = self.seed;
            list.push(MatchRule::from(v));
            Ok(list)
        }

        fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            let mut list = self.seed;
            list.push(MatchRule::from(v));
            Ok(list)
        }
    }

    deserializer.deserialize_any(ListVisitor { seed })
}

pub(crate) fn deserialize_with_match_rules<'de, D, C>(
    seed: Vec<WithMatchRules<C>>,
    deserializer: D,
) -> Result<Vec<WithMatchRules<C>>, D::Error>
where
    D: Deserializer<'de>,
    C: Default + DeserializeMap<'de> + PartialEq + Eq,
{
    deserialize_with_match_rules_custom(seed, deserializer, || C::default().visitor())
}

fn deserialize_with_match_rules_custom<'de, D, V>(
    seed: Vec<WithMatchRules<V::Value>>,
    deserializer: D,
    new_visitor: impl Fn() -> V,
) -> Result<Vec<WithMatchRules<V::Value>>, D::Error>
where
    D: Deserializer<'de>,
    V: MapVisitor<'de>,
    V::Value: PartialEq + Eq,
{
    struct MatchRulesVisitor<'de> {
        key: String,
        inner: <MatchRules as DeserializeMap<'de>>::Visitor,
    }
    impl<'de> DeserializeSeed<'de> for MatchRulesVisitor<'de> {
        type Value = <MatchRules as DeserializeMap<'de>>::Visitor;

        fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            self.inner.visit_field(&self.key, deserializer)
        }
    }

    struct EntryVisitor<V> {
        inner: V,
    }
    impl<'de, V> Visitor<'de> for EntryVisitor<V>
    where
        V: MapVisitor<'de>,
        V::Value: PartialEq + Eq,
    {
        type Value = WithMatchRules<V::Value>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("WithMatchRules<C>")
        }

        fn visit_map<A>(mut self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let mut match_rules = MatchRules::visitor(MatchRules::default());
            let mut seen_include = false;
            let mut seen_exclude = false;
            while let Some(key) = map.next_key::<String>()? {
                match key.as_str() {
                    "include" if !seen_include => {
                        seen_include = true;
                        match_rules = map.next_value_seed(MatchRulesVisitor {
                            key,
                            inner: match_rules,
                        })?;
                    }
                    "exclude" if !seen_exclude => {
                        seen_exclude = true;
                        match_rules = map.next_value_seed(MatchRulesVisitor {
                            key,
                            inner: match_rules,
                        })?;
                    }
                    _ => {
                        struct DeserializeField<T> {
                            key: String,
                            inner: T,
                        }
                        impl<'de, T> DeserializeSeed<'de> for DeserializeField<T>
                        where
                            T: MapVisitor<'de>,
                        {
                            type Value = T;

                            fn deserialize<D>(
                                self,
                                deserializer: D,
                            ) -> Result<Self::Value, D::Error>
                            where
                                D: Deserializer<'de>,
                            {
                                self.inner.visit_field(&self.key, deserializer)
                            }
                        }

                        self.inner = map.next_value_seed(DeserializeField {
                            key,
                            inner: self.inner,
                        })?;
                    }
                }
            }
            Ok(WithMatchRules {
                match_rules: match_rules.finalize()?,
                conf: self.inner.finalize()?,
            })
        }
    }
    impl<'de, V> DeserializeSeed<'de> for EntryVisitor<V>
    where
        V: MapVisitor<'de>,
        V::Value: PartialEq + Eq,
    {
        type Value = <Self as Visitor<'de>>::Value;

        fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_map(self)
        }
    }

    struct ListVisitor<'de, Callback, V>
    where
        V: MapVisitor<'de>,
        V::Value: PartialEq + Eq,
    {
        seed: Vec<WithMatchRules<V::Value>>,
        new_visitor: Callback,
    }
    impl<'de, Callback, V> Visitor<'de> for ListVisitor<'de, Callback, V>
    where
        Callback: Fn() -> V,
        V: MapVisitor<'de>,
        V::Value: PartialEq + Eq,
    {
        type Value = Vec<WithMatchRules<V::Value>>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("WithMatchRules<C> or Vec<WithMatchRules<C>>")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut list = self.seed;
            loop {
                let visitor = EntryVisitor {
                    inner: (self.new_visitor)(),
                };
                if let Some(entry) = seq.next_element_seed(visitor)? {
                    list.push(entry);
                } else {
                    break;
                }
            }
            Ok(list)
        }

        fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let mut list = self.seed;
            let visitor = EntryVisitor {
                inner: (self.new_visitor)(),
            };
            list.push(visitor.visit_map(map)?);
            Ok(list)
        }
    }

    deserializer.deserialize_any(ListVisitor { seed, new_visitor })
}

/// Deserializes `WithMatchRule` wrapping a `HeaderName`/`HeaderValue` map.
pub(crate) fn deserialize_custom_headers<'de, D>(
    seed: Vec<WithMatchRules<HashMap<HeaderName, HeaderValue>>>,
    deserializer: D,
) -> Result<Vec<WithMatchRules<HashMap<HeaderName, HeaderValue>>>, D::Error>
where
    D: Deserializer<'de>,
{
    struct HeadersVisitor {
        headers: HashMap<HeaderName, HeaderValue>,
    }
    impl<'de> MapVisitor<'de> for HeadersVisitor {
        type Value = HashMap<HeaderName, HeaderValue>;

        fn accepts_field(_field: &str) -> bool {
            true
        }

        fn list_fields(_list: &mut Vec<&'static str>) {}

        fn visit_field<D>(mut self, field: &str, deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            let name =
                HeaderName::try_from(field).map_err(|_| D::Error::custom("Invalid header name"))?;
            let value = HeaderValue::try_from(String::deserialize(deserializer)?)
                .map_err(|_| D::Error::custom("Invalid header value"))?;
            self.headers.insert(name, value);
            Ok(self)
        }

        fn finalize<E>(self) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(self.headers)
        }
    }

    deserialize_with_match_rules_custom(seed, deserializer, || HeadersVisitor {
        headers: HashMap::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use module_utils::FromYaml;

    #[test]
    fn path_normalization() {
        assert_eq!(normalize_path(""), "");
        assert_eq!(normalize_path("/"), "");
        assert_eq!(normalize_path("///"), "");
        assert_eq!(normalize_path("abc"), "abc");
        assert_eq!(normalize_path("//abc//"), "abc");
        assert_eq!(normalize_path("abc/def"), "abc/def");
        assert_eq!(normalize_path("//abc//def//"), "abc/def");
    }

    #[test]
    fn match_rule_deserialization() {
        #[derive(Debug, Default, Eq, PartialEq, DeserializeMap)]
        struct Conf {
            #[module_utils(deserialize_with_seed = "deserialize_match_rule_list")]
            rules: Vec<MatchRule>,
        }

        assert_eq!(
            Conf::from_yaml(
                r#"
                rules: []
            "#
            )
            .unwrap(),
            Conf { rules: vec![] }
        );

        assert_eq!(
            Conf::from_yaml(
                r#"
                rules: ""
            "#
            )
            .unwrap(),
            Conf {
                rules: vec![MatchRule {
                    host: "".to_owned(),
                    path: "".to_owned(),
                    prefix: true,
                }],
            }
        );

        assert_eq!(
            Conf::from_yaml(
                r#"
                rules: example.com
            "#
            )
            .unwrap(),
            Conf {
                rules: vec![MatchRule {
                    host: "example.com".to_owned(),
                    path: "".to_owned(),
                    prefix: true,
                }],
            }
        );

        assert_eq!(
            Conf::from_yaml(
                r#"
                rules: /
            "#
            )
            .unwrap(),
            Conf {
                rules: vec![MatchRule {
                    host: "".to_owned(),
                    path: "".to_owned(),
                    prefix: false,
                }],
            }
        );

        assert_eq!(
            Conf::from_yaml(
                r#"
                rules: /*
            "#
            )
            .unwrap(),
            Conf {
                rules: vec![MatchRule {
                    host: "".to_owned(),
                    path: "".to_owned(),
                    prefix: true,
                }],
            }
        );

        assert_eq!(
            Conf::from_yaml(
                r#"
                rules: localhost///subdir//sub//
            "#
            )
            .unwrap(),
            Conf {
                rules: vec![MatchRule {
                    host: "localhost".to_owned(),
                    path: "subdir/sub".to_owned(),
                    prefix: false,
                }],
            }
        );

        assert_eq!(
            Conf::from_yaml(
                r#"
                rules: [/, example.com]
            "#
            )
            .unwrap(),
            Conf {
                rules: vec![
                    MatchRule {
                        host: "".to_owned(),
                        path: "".to_owned(),
                        prefix: false,
                    },
                    MatchRule {
                        host: "example.com".to_owned(),
                        path: "".to_owned(),
                        prefix: true,
                    }
                ],
            }
        );

        assert_eq!(
            Conf::from_yaml(
                r#"
                rules:
                - /
                - example.com
            "#
            )
            .unwrap(),
            Conf {
                rules: vec![
                    MatchRule {
                        host: "".to_owned(),
                        path: "".to_owned(),
                        prefix: false,
                    },
                    MatchRule {
                        host: "example.com".to_owned(),
                        path: "".to_owned(),
                        prefix: true,
                    }
                ],
            }
        );
    }

    #[test]
    fn match_rule_merge() {
        #[derive(Debug, Default, Eq, PartialEq, DeserializeMap)]
        struct Conf {
            #[module_utils(deserialize_with_seed = "deserialize_match_rule_list")]
            rules: Vec<MatchRule>,
        }

        let conf = Conf::from_yaml(
            r#"
                rules: /*
            "#,
        )
        .unwrap();

        let conf = conf
            .merge_from_yaml(
                r#"
                rules: example.com
            "#,
            )
            .unwrap();

        let conf = conf
            .merge_from_yaml(
                r#"
                rules:
                - /subdir
                - example.net/subdir/*
            "#,
            )
            .unwrap();

        assert_eq!(
            conf,
            Conf {
                rules: vec![
                    MatchRule {
                        host: "".to_owned(),
                        path: "".to_owned(),
                        prefix: true,
                    },
                    MatchRule {
                        host: "example.com".to_owned(),
                        path: "".to_owned(),
                        prefix: true,
                    },
                    MatchRule {
                        host: "".to_owned(),
                        path: "subdir".to_owned(),
                        prefix: false,
                    },
                    MatchRule {
                        host: "example.net".to_owned(),
                        path: "subdir".to_owned(),
                        prefix: true,
                    },
                ]
            }
        );
    }

    #[test]
    fn with_match_rules_deserialization() {
        #[derive(Debug, Default, Eq, PartialEq, DeserializeMap)]
        struct DummyInner {
            value: u32,
        }

        #[derive(Debug, Default, Eq, PartialEq, DeserializeMap)]
        struct DummyConf {
            #[module_utils(deserialize_with_seed = "deserialize_with_match_rules")]
            inner: Vec<WithMatchRules<DummyInner>>,
        }

        assert_eq!(
            DummyConf::from_yaml(
                r#"
                    inner:
                        value: 12
                "#
            )
            .unwrap(),
            DummyConf {
                inner: vec![WithMatchRules {
                    match_rules: Default::default(),
                    conf: DummyInner { value: 12 },
                }],
            }
        );

        assert_eq!(
            DummyConf::from_yaml(
                r#"
                    inner:
                        include: /*
                        value: 12
                "#
            )
            .unwrap(),
            DummyConf {
                inner: vec![WithMatchRules {
                    match_rules: MatchRules {
                        include: vec![MatchRule::from("/*")],
                        ..Default::default()
                    },
                    conf: DummyInner { value: 12 },
                }],
            }
        );

        assert_eq!(
            DummyConf::from_yaml(
                r#"
                    inner:
                    - value: 12
                    - value: 34
                "#
            )
            .unwrap(),
            DummyConf {
                inner: vec![
                    WithMatchRules {
                        match_rules: Default::default(),
                        conf: DummyInner { value: 12 },
                    },
                    WithMatchRules {
                        match_rules: Default::default(),
                        conf: DummyInner { value: 34 },
                    }
                ],
            }
        );
    }

    #[test]
    fn custom_headers_deserialization() {
        #[derive(Debug, Default, Eq, PartialEq, DeserializeMap)]
        struct DummyConf {
            #[module_utils(deserialize_with_seed = "deserialize_custom_headers")]
            inner: Vec<WithMatchRules<HashMap<HeaderName, HeaderValue>>>,
        }

        assert_eq!(
            DummyConf::from_yaml(
                r#"
                    inner:
                        X-A: a
                        X-B: b
                "#
            )
            .unwrap(),
            DummyConf {
                inner: vec![WithMatchRules {
                    match_rules: Default::default(),
                    conf: HashMap::from([
                        ("x-a".try_into().unwrap(), "a".try_into().unwrap()),
                        ("x-b".try_into().unwrap(), "b".try_into().unwrap())
                    ]),
                }],
            }
        );

        assert_eq!(
            DummyConf::from_yaml(
                r#"
                    inner:
                        include: /*
                        X-A: a
                        X-B: b
                "#
            )
            .unwrap(),
            DummyConf {
                inner: vec![WithMatchRules {
                    match_rules: MatchRules {
                        include: vec![MatchRule::from("/*")],
                        ..Default::default()
                    },
                    conf: HashMap::from([
                        ("x-a".try_into().unwrap(), "a".try_into().unwrap()),
                        ("x-b".try_into().unwrap(), "b".try_into().unwrap())
                    ]),
                }],
            }
        );

        assert_eq!(
            DummyConf::from_yaml(
                r#"
                    inner:
                        include: /*
                        X-A: a
                        X-B: b
                        include: value
                "#
            )
            .unwrap(),
            DummyConf {
                inner: vec![WithMatchRules {
                    match_rules: MatchRules {
                        include: vec![MatchRule::from("/*")],
                        ..Default::default()
                    },
                    conf: HashMap::from([
                        ("x-a".try_into().unwrap(), "a".try_into().unwrap()),
                        ("x-b".try_into().unwrap(), "b".try_into().unwrap()),
                        ("include".try_into().unwrap(), "value".try_into().unwrap())
                    ]),
                }],
            }
        );

        assert_eq!(
            DummyConf::from_yaml(
                r#"
                    inner:
                    -
                        X-A: a
                        X-B: b
                    -
                        include: /*
                        include: value
                "#
            )
            .unwrap(),
            DummyConf {
                inner: vec![
                    WithMatchRules {
                        match_rules: Default::default(),
                        conf: HashMap::from([
                            ("x-a".try_into().unwrap(), "a".try_into().unwrap()),
                            ("x-b".try_into().unwrap(), "b".try_into().unwrap()),
                        ]),
                    },
                    WithMatchRules {
                        match_rules: MatchRules {
                            include: vec![MatchRule::from("/*")],
                            ..Default::default()
                        },
                        conf: HashMap::from([(
                            "include".try_into().unwrap(),
                            "value".try_into().unwrap()
                        )]),
                    },
                ],
            }
        );
    }

    #[test]
    fn with_match_rules_deserialization_merging() {
        #[derive(Debug, Default, Eq, PartialEq, DeserializeMap)]
        struct DummyInner {
            value: u32,
        }

        #[derive(Debug, Default, Eq, PartialEq, DeserializeMap)]
        struct DummyConf {
            #[module_utils(deserialize_with_seed = "deserialize_with_match_rules")]
            inner: Vec<WithMatchRules<DummyInner>>,
        }

        let conf = DummyConf::from_yaml(
            r#"
                inner:
                    value: 12
            "#,
        )
        .unwrap();
        let conf = conf
            .merge_from_yaml(
                r#"
                inner:
                    value: 34
            "#,
            )
            .unwrap();

        assert_eq!(
            conf,
            DummyConf {
                inner: vec![
                    WithMatchRules {
                        match_rules: Default::default(),
                        conf: DummyInner { value: 12 },
                    },
                    WithMatchRules {
                        match_rules: Default::default(),
                        conf: DummyInner { value: 34 },
                    }
                ],
            }
        );
    }
}
