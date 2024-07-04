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

use async_trait::async_trait;
use pandora_module_utils::pingora::{
    create_test_session, Error, ErrorType, RequestHeader, SessionWrapper,
};
use pandora_module_utils::serde::{Deserialize, Deserializer};
use pandora_module_utils::{
    merge_conf, DeserializeMap, FromYaml, RequestFilter, RequestFilterResult,
};
use startup_module::DefaultApp;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Debug;
use test_log::test;

#[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
struct Handler1Conf {
    handle_request: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Handler1 {
    handle_request: bool,
}

impl TryFrom<Handler1Conf> for Handler1 {
    type Error = Box<Error>;

    fn try_from(conf: Handler1Conf) -> Result<Self, Self::Error> {
        Ok(Self {
            handle_request: conf.handle_request,
        })
    }
}

#[async_trait]
impl RequestFilter for Handler1 {
    type Conf = Handler1Conf;
    type CTX = ();

    fn new_ctx() -> Self::CTX {}

    async fn request_filter(
        &self,
        _session: &mut (impl SessionWrapper),
        _ctx: &mut Self::CTX,
    ) -> Result<RequestFilterResult, Box<Error>> {
        Ok(if self.handle_request {
            RequestFilterResult::ResponseSent
        } else {
            RequestFilterResult::Unhandled
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, DeserializeMap)]
struct Handler2Conf<T: Default + Sync, U>
where
    U: Default + Sync,
{
    value1: T,
    value2: U,
    value3: u32,
}

impl<T: Default + Sync, U: Default + Sync> Default for Handler2Conf<T, U> {
    fn default() -> Self {
        Self {
            value1: T::default(),
            value2: U::default(),
            value3: 1234,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Handler2<T: Default + Sync, U: Default + Sync> {
    conf: Handler2Conf<T, U>,
}

impl<T: Default + Sync, U: Default + Sync> TryFrom<Handler2Conf<T, U>> for Handler2<T, U> {
    type Error = Box<Error>;

    fn try_from(conf: Handler2Conf<T, U>) -> Result<Self, Self::Error> {
        Ok(Self { conf })
    }
}

struct Handler2Ctx {
    value1: u32,
    value2: String,
}

#[async_trait]
impl<T: Default + Sync, U: Default + Sync> RequestFilter for Handler2<T, U> {
    type Conf = Handler2Conf<T, U>;
    type CTX = Handler2Ctx;

    fn new_ctx() -> Self::CTX {
        Self::CTX {
            value1: 4321,
            value2: "Hi!".into(),
        }
    }

    async fn request_filter(
        &self,
        _session: &mut (impl SessionWrapper),
        ctx: &mut Self::CTX,
    ) -> Result<RequestFilterResult, Box<Error>> {
        ctx.value1 = self.conf.value3;
        Ok(RequestFilterResult::Unhandled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, RequestFilter)]
struct Handler<T: Default + Sync, U>
where
    U: Default + Sync,
{
    handler2: Handler2<T, U>,
    handler1: Handler1,
}

#[test]
fn request_filter() {
    let conf = <Handler<String, u32> as RequestFilter>::Conf::default();
    assert_eq!(conf.handler1.handle_request, bool::default());
    assert_eq!(conf.handler2.value1, String::default());
    assert_eq!(conf.handler2.value2, u32::default());
    assert_eq!(conf.handler2.value3, 1234u32);

    let ctx = <Handler<String, u32> as RequestFilter>::new_ctx();
    assert_eq!(ctx.handler2.value1, 4321u32);
    assert_eq!(ctx.handler2.value2, String::from("Hi!"));

    let conf = <Handler<String, u32> as RequestFilter>::Conf::from_yaml(
        r"#
            handle_request: true
            value1: Hi there
            value2: 5678
            value3: 8765
        #",
    )
    .expect("configuration should load");
    assert!(conf.handler1.handle_request);
    assert_eq!(conf.handler2.value1, String::from("Hi there"));
    assert_eq!(conf.handler2.value2, 5678);
    assert_eq!(conf.handler2.value3, 8765);

    <Handler<String, u32> as RequestFilter>::Conf::from_yaml(
        r"#
            unknown_field: flagged
        #",
    )
    .expect_err("unknown configuration field should be rejected");
}

#[test(tokio::test)]
async fn handler() -> Result<(), Box<Error>> {
    let header = RequestHeader::build("GET", "/".as_bytes(), None)?;
    let session = create_test_session(header).await;

    let conf = <Handler<String, u32> as RequestFilter>::Conf::default();
    let handler = Handler::<String, u32>::try_from(conf).unwrap();
    let mut app = DefaultApp::new(handler);

    let result = app.handle_request(session).await;
    assert_eq!(
        result.err().as_ref().map(|err| &err.etype),
        Some(&ErrorType::HTTPStatus(404))
    );

    let header = RequestHeader::build("GET", "/".as_bytes(), None)?;
    let session = create_test_session(header).await;

    let conf = <Handler<String, u32> as RequestFilter>::Conf::default();
    let mut handler = Handler::<String, u32>::try_from(conf).unwrap();
    handler.handler1.handle_request = true;
    let mut app = DefaultApp::new(handler);

    let result = app.handle_request(session).await;
    assert!(result.err().is_none());

    Ok(())
}

#[test]
fn container_attributes() {
    #[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
    #[pandora(rename_all = "kebab-case")]
    struct Conf1 {
        value: String,
        string_value: String,
        #[pandora(rename = "string_value2")]
        string_value2: String,
    }

    #[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
    #[pandora(rename_all(deserialize = "kebab-case"))]
    struct Conf2 {
        value: String,
        string_value: String,
        #[pandora(rename = "string_value2")]
        string_value2: String,
    }

    let conf = Conf1::from_yaml(
        r#"
            value: "1"
            string-value: "2"
            string_value2: "3"
        "#,
    )
    .unwrap();
    assert_eq!(&conf.value, "1");
    assert_eq!(&conf.string_value, "2");
    assert_eq!(&conf.string_value2, "3");

    let conf = Conf2::from_yaml(
        r#"
            value: "1"
            string-value: "2"
            string_value2: "3"
        "#,
    )
    .unwrap();
    assert_eq!(&conf.value, "1");
    assert_eq!(&conf.string_value, "2");
    assert_eq!(&conf.string_value2, "3");
}

#[test]
fn field_attributes() {
    use pandora_module_utils::serde::{de::Deserializer, Deserialize};

    #[derive(Debug, Clone, Default, PartialEq, Eq)]
    struct Blub {
        value: String,
    }

    impl Blub {
        fn deserialize<'de, D>(deserializer: D) -> Result<Blub, D::Error>
        where
            D: Deserializer<'de>,
        {
            Ok(Blub {
                value: String::deserialize(deserializer)?,
            })
        }
    }

    #[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
    struct Blob {
        value: String,
    }

    fn custom_deserialize<'de, D>(deserializer: D) -> Result<Blub, D::Error>
    where
        D: Deserializer<'de>,
    {
        Blub::deserialize(deserializer)
    }

    #[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
    struct Conf {
        #[pandora(rename = "v1", alias = "hi1")]
        #[pandora(alias = "another1")]
        value1: u32,
        #[pandora(skip)]
        value2: Option<Blub>,
        #[pandora(deserialize_with = "custom_deserialize", alias = "v3")]
        value3: Blub,
        #[pandora(with = "Blub", rename(deserialize = "v4"))]
        value4: Blub,
        #[pandora(skip_deserializing)]
        value5: Option<Blub>,
        #[pandora(flatten)]
        value6: Blob,
    }

    let conf = Conf::from_yaml(
        r#"
            v1: 12
            value3: "hi!"
            v4: "another hi!"
        "#,
    )
    .unwrap();
    assert_eq!(conf.value1, 12);
    assert!(conf.value2.is_none());
    assert_eq!(conf.value3.value, "hi!".to_owned());
    assert_eq!(conf.value4.value, "another hi!".to_owned());
    assert!(conf.value5.is_none());
    assert_eq!(conf.value6.value, String::new());

    Conf::from_yaml("value1: renamed").unwrap_err();
    Conf::from_yaml("value2: skipped").unwrap_err();
    Conf::from_yaml("value4: renamed").unwrap_err();
    Conf::from_yaml("value5: skipped").unwrap_err();
    Conf::from_yaml("value6: flattened").unwrap_err();

    let conf = Conf::from_yaml(
        r#"
            hi1: 34
            v3: alias
            value: v6
        "#,
    )
    .unwrap();
    assert_eq!(conf.value1, 34);
    assert!(conf.value2.is_none());
    assert_eq!(conf.value3.value, "alias".to_owned());
    assert_eq!(conf.value4.value, String::new());
    assert!(conf.value5.is_none());
    assert_eq!(conf.value6.value, "v6".to_owned());

    let conf = Conf::from_yaml("another1: 56").unwrap();
    assert_eq!(conf.value1, 56);
    assert!(conf.value2.is_none());
    assert_eq!(conf.value3.value, String::new());
    assert_eq!(conf.value4.value, String::new());
    assert!(conf.value5.is_none());
    assert_eq!(conf.value6.value, String::new());
}

#[test]
fn from_yaml_seed() {
    fn assert_hash_eq<V: Debug + Eq>(left: &HashMap<String, V>, right: Vec<(&str, V)>) {
        let right = HashMap::from_iter(right.into_iter().map(|(k, v)| (k.to_owned(), v)));
        assert_eq!(left, &right);
    }

    fn custom_deserialize_seed<'de, D>(
        mut seed: String,
        deserializer: D,
    ) -> Result<String, D::Error>
    where
        D: Deserializer<'de>,
    {
        let other = String::deserialize(deserializer)?;
        seed.push_str(&other);
        Ok(seed)
    }

    #[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
    struct Conf1 {
        value1: HashMap<String, u32>,
        value2: u32,
    }

    #[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
    struct Conf2 {
        value3: Vec<bool>,
        #[pandora(deserialize_with_seed = "custom_deserialize_seed")]
        value4: String,
    }

    let conf = Conf1::from_yaml(
        r#"
            value1:
                hi: 1234
            value2: 12
        "#,
    )
    .unwrap();
    assert_hash_eq(&conf.value1, vec![("hi", 1234)]);
    assert_eq!(conf.value2, 12);

    let conf = conf.merge_from_yaml("value2: 34").unwrap();
    assert_hash_eq(&conf.value1, vec![("hi", 1234)]);
    assert_eq!(conf.value2, 34);

    let conf = conf.merge_from_yaml("value1: {not hi: 4321}").unwrap();
    assert_hash_eq(&conf.value1, vec![("hi", 1234), ("not hi", 4321)]);
    assert_eq!(conf.value2, 34);

    {
        #[merge_conf]
        struct Conf {
            conf1: Conf1,
            conf2: Conf2,
        }

        let conf = Conf::from_yaml(
            r#"
                value1:
                    hi: 1234
                value2: 12
            "#,
        )
        .unwrap();
        assert_hash_eq(&conf.conf1.value1, vec![("hi", 1234)]);
        assert_eq!(conf.conf1.value2, 12);
        assert_eq!(conf.conf2.value3, Vec::new());
        assert_eq!(conf.conf2.value4, String::new());

        let conf = conf.merge_from_yaml("value3: [true, false]").unwrap();
        assert_hash_eq(&conf.conf1.value1, vec![("hi", 1234)]);
        assert_eq!(conf.conf1.value2, 12);
        assert_eq!(conf.conf2.value3, vec![true, false]);
        assert_eq!(conf.conf2.value4, String::new());

        let conf = conf.merge_from_yaml("value4: hi").unwrap();
        assert_hash_eq(&conf.conf1.value1, vec![("hi", 1234)]);
        assert_eq!(conf.conf1.value2, 12);
        assert_eq!(conf.conf2.value3, vec![true, false]);
        assert_eq!(conf.conf2.value4, "hi".to_owned());

        let conf = conf
            .merge_from_yaml(
                r#"
                value1:
                    hi: 4321
                value2: 34
                value3: [false]
                value4: _addendum
            "#,
            )
            .unwrap();
        assert_hash_eq(&conf.conf1.value1, vec![("hi", 4321)]);
        assert_eq!(conf.conf1.value2, 34);
        assert_eq!(conf.conf2.value3, vec![true, false, false]);
        assert_eq!(conf.conf2.value4, "hi_addendum".to_owned());
    }

    {
        #[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
        struct InnerConf {
            value: Vec<bool>,
        }

        #[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
        struct Conf {
            map: HashMap<String, InnerConf>,
        }

        let conf = Conf::from_yaml(
            r#"
                map:
                    hi:
                        value: [true]
            "#,
        )
        .unwrap();
        assert_hash_eq(&conf.map, vec![("hi", InnerConf { value: vec![true] })]);

        let conf = conf
            .merge_from_yaml(
                r#"
                map:
                    not hi:
                        value: []
            "#,
            )
            .unwrap();
        assert_hash_eq(
            &conf.map,
            vec![
                ("hi", InnerConf { value: vec![true] }),
                ("not hi", InnerConf { value: Vec::new() }),
            ],
        );

        let conf = conf
            .merge_from_yaml(
                r#"
                map:
                    hi:
                        value: [false]
                    not hi:
                        value: [false]
            "#,
            )
            .unwrap();
        assert_hash_eq(
            &conf.map,
            vec![
                (
                    "hi",
                    InnerConf {
                        value: vec![true, false],
                    },
                ),
                ("not hi", InnerConf { value: vec![false] }),
            ],
        );
    }
}

#[test]
fn merge_across_maps() {
    fn assert_hashmap_eq<V: Debug + Eq>(left: &HashMap<String, V>, right: Vec<(&str, V)>) {
        let right = HashMap::from_iter(right.into_iter().map(|(k, v)| (k.to_owned(), v)));
        assert_eq!(left, &right);
    }

    fn assert_btreemap_eq<V: Debug + Eq>(left: &BTreeMap<String, V>, right: Vec<(&str, V)>) {
        let right = BTreeMap::from_iter(right.into_iter().map(|(k, v)| (k.to_owned(), v)));
        assert_eq!(left, &right);
    }

    #[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
    struct ConfInner {
        value1: u32,
        value2: u32,
    }

    #[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
    struct Conf {
        map1: HashMap<String, ConfInner>,
        map2: BTreeMap<String, ConfInner>,
    }

    let conf = Conf::from_yaml(
        r#"
            map1:
                hi:
                    value1: 12
            map2:
                hi:
                    value1: 12
        "#,
    )
    .unwrap();
    assert_hashmap_eq(
        &conf.map1,
        vec![(
            "hi",
            ConfInner {
                value1: 12,
                value2: 0,
            },
        )],
    );
    assert_btreemap_eq(
        &conf.map2,
        vec![(
            "hi",
            ConfInner {
                value1: 12,
                value2: 0,
            },
        )],
    );

    let conf = conf
        .merge_from_yaml(
            r#"
                map1:
                    hi:
                        value2: 34
                    another:
                        value2: 56
                map2:
                    hi:
                        value2: 34
                    another:
                        value2: 56
            "#,
        )
        .unwrap();
    assert_hashmap_eq(
        &conf.map1,
        vec![
            (
                "hi",
                ConfInner {
                    value1: 12,
                    value2: 34,
                },
            ),
            (
                "another",
                ConfInner {
                    value1: 0,
                    value2: 56,
                },
            ),
        ],
    );
    assert_btreemap_eq(
        &conf.map2,
        vec![
            (
                "hi",
                ConfInner {
                    value1: 12,
                    value2: 34,
                },
            ),
            (
                "another",
                ConfInner {
                    value1: 0,
                    value2: 56,
                },
            ),
        ],
    );

    let conf = conf
        .merge_from_yaml(
            r#"
                map1:
                    hi:
                        value1: 78
                map2:
                    hi:
                        value1: 78
            "#,
        )
        .unwrap();
    assert_hashmap_eq(
        &conf.map1,
        vec![
            (
                "hi",
                ConfInner {
                    value1: 78,
                    value2: 34,
                },
            ),
            (
                "another",
                ConfInner {
                    value1: 0,
                    value2: 56,
                },
            ),
        ],
    );
    assert_btreemap_eq(
        &conf.map2,
        vec![
            (
                "hi",
                ConfInner {
                    value1: 78,
                    value2: 34,
                },
            ),
            (
                "another",
                ConfInner {
                    value1: 0,
                    value2: 56,
                },
            ),
        ],
    );
}

#[test]
fn deserialize_merge_across_options() {
    #[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
    struct ConfInner {
        value1: Option<u32>,
        value2: Option<u32>,
    }

    #[derive(Debug, Default, Clone, PartialEq, Eq, DeserializeMap)]
    struct Conf {
        option: Option<ConfInner>,
    }

    let conf = Conf { option: None };

    let conf = conf
        .merge_from_yaml(
            r#"
                option:
                    value2: 12
            "#,
        )
        .unwrap();
    assert_eq!(
        conf,
        Conf {
            option: Some(ConfInner {
                value1: None,
                value2: Some(12),
            })
        }
    );

    let conf = conf
        .merge_from_yaml(
            r#"
                option:
                    value1: 34
            "#,
        )
        .unwrap();
    assert_eq!(
        conf,
        Conf {
            option: Some(ConfInner {
                value1: Some(34),
                value2: Some(12),
            })
        }
    );

    let conf = conf.merge_from_yaml("{}").unwrap();
    assert_eq!(
        conf,
        Conf {
            option: Some(ConfInner {
                value1: Some(34),
                value2: Some(12),
            })
        }
    );

    let conf = conf
        .merge_from_yaml(
            r#"
                option:
                    value2: 56
            "#,
        )
        .unwrap();
    assert_eq!(
        conf,
        Conf {
            option: Some(ConfInner {
                value1: Some(34),
                value2: Some(56),
            })
        }
    );
}
