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
use module_utils::pingora::{Error, RequestHeader, SessionWrapper, TestSession};
use module_utils::{merge_conf, DeserializeMap, FromYaml, RequestFilter, RequestFilterResult};
use std::collections::HashMap;
use std::fmt::Debug;
use test_log::test;

#[derive(Debug, Default, DeserializeMap)]
struct Handler1Conf {
    handle_request: bool,
}

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
            RequestFilterResult::Handled
        } else {
            RequestFilterResult::Unhandled
        })
    }
}

#[derive(Debug, DeserializeMap)]
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

#[derive(RequestFilter)]
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
    let mut session = TestSession::from(header).await;

    let conf = <Handler<String, u32> as RequestFilter>::Conf::default();
    let mut ctx = <Handler<String, u32> as RequestFilter>::new_ctx();
    let mut handler = Handler::<String, u32>::try_from(conf).unwrap();

    assert_eq!(
        handler.request_filter(&mut session, &mut ctx).await?,
        RequestFilterResult::Unhandled
    );

    handler.handler1.handle_request = true;
    assert_eq!(
        handler.request_filter(&mut session, &mut ctx).await?,
        RequestFilterResult::Handled
    );

    Ok(())
}

#[test]
fn field_attributes() {
    use module_utils::serde::{de::Deserializer, Deserialize};

    #[derive(Debug, Default)]
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

    fn custom_deserialize<'de, D>(deserializer: D) -> Result<Blub, D::Error>
    where
        D: Deserializer<'de>,
    {
        Blub::deserialize(deserializer)
    }

    #[derive(Debug, Default, DeserializeMap)]
    struct Conf {
        #[module_utils(rename = "v1", alias = "hi1")]
        #[module_utils(alias = "another1")]
        value1: u32,
        #[module_utils(skip)]
        value2: Option<Blub>,
        #[module_utils(deserialize_with = "custom_deserialize", alias = "v3")]
        value3: Blub,
        #[module_utils(with = "Blub", rename(deserialize = "v4"))]
        value4: Blub,
        #[module_utils(skip_deserializing)]
        value5: Option<Blub>,
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

    Conf::from_yaml("value1: renamed").unwrap_err();
    Conf::from_yaml("value2: skipped").unwrap_err();
    Conf::from_yaml("value4: renamed").unwrap_err();
    Conf::from_yaml("value5: skipped").unwrap_err();

    let conf = Conf::from_yaml(
        r#"
            hi1: 34
            v3: alias
        "#,
    )
    .unwrap();
    assert_eq!(conf.value1, 34);
    assert!(conf.value2.is_none());
    assert_eq!(conf.value3.value, "alias".to_owned());
    assert_eq!(conf.value4.value, String::new());
    assert!(conf.value5.is_none());

    let conf = Conf::from_yaml("another1: 56").unwrap();
    assert_eq!(conf.value1, 56);
    assert!(conf.value2.is_none());
    assert_eq!(conf.value3.value, String::new());
    assert_eq!(conf.value4.value, String::new());
    assert!(conf.value5.is_none());
}

#[test]
fn from_yaml_seed() {
    fn assert_hash_eq<V: Debug + Eq>(left: &HashMap<String, V>, right: Vec<(&str, V)>) {
        let right = HashMap::from_iter(right.into_iter().map(|(k, v)| (k.to_owned(), v)));
        assert_eq!(left, &right);
    }

    #[derive(Debug, Default, PartialEq, Eq, DeserializeMap)]
    struct Conf1 {
        value1: HashMap<String, u32>,
        value2: u32,
    }

    #[derive(Debug, Default, PartialEq, Eq, DeserializeMap)]
    struct Conf2 {
        value3: Vec<bool>,
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

        let conf = conf.merge_from_yaml("value3: [true, false]").unwrap();
        assert_hash_eq(&conf.conf1.value1, vec![("hi", 1234)]);
        assert_eq!(conf.conf1.value2, 12);
        assert_eq!(conf.conf2.value3, vec![true, false]);

        let conf = conf
            .merge_from_yaml(
                r#"
                value1:
                    hi: 4321
                value2: 34
                value3: [false]
            "#,
            )
            .unwrap();
        assert_hash_eq(&conf.conf1.value1, vec![("hi", 4321)]);
        assert_eq!(conf.conf1.value2, 34);
        assert_eq!(conf.conf2.value3, vec![true, false, false]);
    }

    {
        #[derive(Debug, Default, DeserializeMap)]
        struct Conf {
            map: HashMap<String, Conf2>,
        }

        let conf = Conf::from_yaml(
            r#"
                map:
                    hi:
                        value3: [true]
            "#,
        )
        .unwrap();
        assert_hash_eq(&conf.map, vec![("hi", Conf2 { value3: vec![true] })]);

        let conf = conf
            .merge_from_yaml(
                r#"
                map:
                    not hi:
                        value3: []
            "#,
            )
            .unwrap();
        assert_hash_eq(
            &conf.map,
            vec![
                ("hi", Conf2 { value3: vec![true] }),
                ("not hi", Conf2 { value3: Vec::new() }),
            ],
        );

        let conf = conf
            .merge_from_yaml(
                r#"
                map:
                    hi:
                        value3: [false]
                    not hi:
                        value3: [false]
            "#,
            )
            .unwrap();
        assert_hash_eq(
            &conf.map,
            vec![
                (
                    "hi",
                    Conf2 {
                        value3: vec![true, false],
                    },
                ),
                (
                    "not hi",
                    Conf2 {
                        value3: vec![false],
                    },
                ),
            ],
        );
    }
}
