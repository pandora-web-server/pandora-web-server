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
use serde::de::{Deserialize, DeserializeSeed, Deserializer, Error as _, MapAccess, Visitor};
use std::collections::HashMap;

use crate::configuration::CustomHeadersConf;

impl<'de> DeserializeSeed<'de> for CustomHeadersConf {
    type Value = Self;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct VisitorImpl {
            inner: CustomHeadersVisitor,
        }

        impl<'de> Visitor<'de> for VisitorImpl {
            type Value = CustomHeadersConf;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct CustomHeadersConf")
            }

            fn visit_map<A>(mut self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                struct DeserializeSeedImpl {
                    key: String,
                    inner: CustomHeadersVisitor,
                }
                impl<'de> DeserializeSeed<'de> for DeserializeSeedImpl {
                    type Value = CustomHeadersVisitor;
                    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
                    where
                        D: Deserializer<'de>,
                    {
                        self.inner.visit_field(&self.key, deserializer)
                    }
                }

                while let Some(key) = map.next_key::<String>()? {
                    self.inner = map.next_value_seed(DeserializeSeedImpl {
                        key,
                        inner: self.inner,
                    })?;
                }

                self.inner.finalize()
            }
        }

        deserializer.deserialize_map(VisitorImpl {
            inner: self.visitor(),
        })
    }
}

impl<'de> Deserialize<'de> for CustomHeadersConf {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        CustomHeadersConf::default().deserialize(deserializer)
    }
}

impl DeserializeMap<'_> for CustomHeadersConf {
    type Visitor = CustomHeadersVisitor;

    fn visitor(self) -> Self::Visitor {
        CustomHeadersVisitor {
            headers: self.headers,
        }
    }
}

#[doc(hidden)]
#[derive(Debug)]
pub struct CustomHeadersVisitor {
    headers: HashMap<HeaderName, HeaderValue>,
}
impl<'de> MapVisitor<'de> for CustomHeadersVisitor {
    type Value = CustomHeadersConf;

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
        Ok(CustomHeadersConf {
            headers: self.headers,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::configuration::{MatchRules, WithMatchRules};

    use super::*;

    use module_utils::{merger::HostPathMatcher, FromYaml, OneOrMany};

    #[test]
    fn custom_headers_deserialization() {
        #[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
        struct DummyConf {
            inner: OneOrMany<WithMatchRules<CustomHeadersConf>>,
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
                    conf: CustomHeadersConf {
                        headers: HashMap::from([
                            ("x-a".try_into().unwrap(), "a".try_into().unwrap()),
                            ("x-b".try_into().unwrap(), "b".try_into().unwrap())
                        ]),
                    }
                }]
                .into(),
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
                        include: vec![HostPathMatcher::from("/*")].into(),
                        ..Default::default()
                    },
                    conf: CustomHeadersConf {
                        headers: HashMap::from([
                            ("x-a".try_into().unwrap(), "a".try_into().unwrap()),
                            ("x-b".try_into().unwrap(), "b".try_into().unwrap())
                        ]),
                    }
                }]
                .into(),
            }
        );

        assert_eq!(
            DummyConf::from_yaml(
                r#"
                    inner:
                        include: /*
                        X-A: a
                        X-B: b
                        Include: value
                "#
            )
            .unwrap(),
            DummyConf {
                inner: vec![WithMatchRules {
                    match_rules: MatchRules {
                        include: vec![HostPathMatcher::from("/*")].into(),
                        ..Default::default()
                    },
                    conf: CustomHeadersConf {
                        headers: HashMap::from([
                            ("x-a".try_into().unwrap(), "a".try_into().unwrap()),
                            ("x-b".try_into().unwrap(), "b".try_into().unwrap()),
                            ("include".try_into().unwrap(), "value".try_into().unwrap())
                        ]),
                    }
                }]
                .into(),
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
                        Include: value
                "#
            )
            .unwrap(),
            DummyConf {
                inner: vec![
                    WithMatchRules {
                        match_rules: Default::default(),
                        conf: CustomHeadersConf {
                            headers: HashMap::from([
                                ("x-a".try_into().unwrap(), "a".try_into().unwrap()),
                                ("x-b".try_into().unwrap(), "b".try_into().unwrap()),
                            ])
                        },
                    },
                    WithMatchRules {
                        match_rules: MatchRules {
                            include: vec![HostPathMatcher::from("/*")].into(),
                            ..Default::default()
                        },
                        conf: CustomHeadersConf {
                            headers: HashMap::from([(
                                "include".try_into().unwrap(),
                                "value".try_into().unwrap()
                            )]),
                        }
                    },
                ]
                .into(),
            }
        );
    }
}
