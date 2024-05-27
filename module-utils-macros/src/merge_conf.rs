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

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, ToTokens};
use syn::parse::{Parse, ParseStream, Parser};
use syn::{Attribute, Data, DeriveInput, Error, Fields, FieldsNamed, GenericParam, Meta, Token};

struct MergeConfParams {
    deny_unknown_fields: bool,
}

impl Parse for MergeConfParams {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut deny_unknown_fields = false;

        for param in input.parse_terminated(Meta::parse, Token![,])? {
            if let Meta::Path(path) = param {
                if path.is_ident("deny_unknown_fields") {
                    deny_unknown_fields = true;
                } else {
                    return Err(Error::new_spanned(path, "unknown parameter"));
                }
            } else {
                return Err(Error::new_spanned(param, "boolean parameter expected"));
            }
        }

        Ok(Self {
            deny_unknown_fields,
        })
    }
}

fn retrieve_field_names(fields: &FieldsNamed) -> TokenStream2 {
    let field_type = fields
        .named
        .iter()
        .map(|field| &field.ty)
        .collect::<Vec<_>>();

    quote! {
        {
            #[derive(::std::fmt::Debug)]
            struct FieldNamesList {
                names: ::std::vec::Vec<String>,
            }

            impl FieldNamesList {
                fn new() -> Self {
                    Self {
                        names: ::std::vec::Vec::new(),
                    }
                }
            }

            impl ::std::fmt::Display for FieldNamesList {
                fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str("not an actual error")
                }
            }

            impl ::std::error::Error for FieldNamesList {}

            impl ::module_utils::serde::de::Error for FieldNamesList {
                fn custom<T>(msg: T) -> Self
                where
                    T: std::fmt::Display
                {
                    const HACK_PREFIX: &str = "field names\n";
                    let msg = msg.to_string();
                    if let Some(suffix) = msg.strip_prefix(HACK_PREFIX) {
                        Self {
                            names: suffix.split('\n').map(|s| s.to_owned()).collect(),
                        }
                    } else {
                        Self::new()
                    }
                }
            }

            struct FieldNamesRetriever {}

            impl<'de> ::module_utils::serde::Deserializer<'de> for FieldNamesRetriever
            {
                type Error = FieldNamesList;

                #[inline(always)]
                fn deserialize_any<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
                where
                    V: ::module_utils::serde::de::Visitor<'de>
                {
                    Err(Self::Error::new())
                }

                ::module_utils::serde::forward_to_deserialize_any! [
                    bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string bytes byte_buf
                    option unit unit_struct newtype_struct seq tuple tuple_struct map enum
                    identifier ignored_any
                ];

                #[inline(always)]
                fn deserialize_struct<V>(
                    mut self,
                    _name: &'static ::std::primitive::str,
                    fields: &'static [&'static ::std::primitive::str],
                    _visitor: V
                ) -> Result<V::Value, Self::Error>
                where
                    V: ::module_utils::serde::de::Visitor<'de>
                {
                    Err(FieldNamesList {
                        names: fields.iter().map(|s| (*s).to_owned()).collect()
                    })
                }
            }

            let mut field_names = ::std::vec::Vec::new();
            #(
                if let Err(field_list) = <#field_type>::deserialize(FieldNamesRetriever {}) {
                    for name in field_list.names {
                        if !field_names.contains(&name) {
                            field_names.push(name);
                        }
                    }
                }
            )*
            field_names.sort();
            field_names
        }
    }
}

fn generate_map_visitor(params: &MergeConfParams) -> TokenStream2 {
    if params.deny_unknown_fields {
        quote! {
            {
                struct Visitor<'a> {
                    names: &'a ::std::vec::Vec<::std::string::String>,
                }

                impl<'de, 'a> ::module_utils::serde::de::Visitor<'de> for Visitor<'a> {
                    type Value = ::std::vec::Vec::<
                        (::std::string::String, ::module_utils::serde_yaml::Value)
                    >;

                    fn expecting(&self, f: &mut ::std::fmt::Formatter<'_>) -> std::fmt::Result {
                        f.write_str("a map")
                    }

                    fn visit_map<A>(self, mut map: A) -> std::result::Result<Self::Value, A::Error>
                    where
                        A: ::module_utils::serde::de::MapAccess<'de>
                    {
                        let mut result = Vec::new();
                        while let Some(key) = map.next_key::<String>()? {
                            if self.names.binary_search(&key).is_err() {
                                // Error::unknown_field() won't accept non-static slices, so we
                                // duplicate its functionality here.
                                return Err(A::Error::custom(::std::format_args!(
                                    "unknown field `{key}`, expected one of `{}`",
                                    self.names.join("`, `"),
                                )));
                            }
                            result.push((key, map.next_value()?));
                        }
                        Ok(result)
                    }
                }

                Visitor {
                    names: &field_names,
                }
            }
        }
    } else {
        quote! {
            {
                struct Visitor {}

                impl<'de> ::module_utils::serde::de::Visitor<'de> for Visitor {
                    type Value = ::std::vec::Vec::<
                        (::std::string::String, ::module_utils::serde_yaml::Value)
                    >;

                    fn expecting(&self, f: &mut ::std::fmt::Formatter<'_>) -> std::fmt::Result {
                        f.write_str("a map")
                    }

                    fn visit_map<A>(self, mut map: A) -> std::result::Result<Self::Value, A::Error>
                    where
                        A: ::module_utils::serde::de::MapAccess<'de>
                    {
                        let mut result = Vec::new();
                        while let Some(key) = map.next_key::<String>()? {
                            result.push((key, map.next_value()?));
                        }
                        Ok(result)
                    }
                }

                Visitor {}
            }
        }
    }
}

fn deserialize_map(fields: &FieldsNamed, params: &MergeConfParams) -> TokenStream2 {
    let retrieve_field_names = retrieve_field_names(fields);
    let visitor = generate_map_visitor(params);

    quote! {
        {
            let field_names = #retrieve_field_names;
            let visitor = #visitor;
            let fields = match deserializer.deserialize_map(visitor) {
                Ok(fields) => fields,
                Err(err) if err.to_string() == "not an actual error" => {
                    // This is our error type, an upper-level merged conf trying to determine
                    // our fields. Make sure to pass on the field names. We cannot do it via
                    // deserialize_struct() because it only takes static slices whereas ours is
                    // dynamic. So this hack will call Error::custom() instead.
                    return Err(D::Error::custom(format!(
                        "field names\n{}",
                        field_names.join("\n"),
                    )));
                }
                Err(err) => return Err(err),
            };
            fields
        }
    }
}

fn generate_deserialize_impl(
    input: &DeriveInput,
    fields: &FieldsNamed,
    params: &MergeConfParams,
) -> TokenStream2 {
    let deserialize_map = deserialize_map(fields, params);

    // This is a custom serde::Deserialize implementation. Normally, using
    // #[serde(flatten)] would be sufficient to produce the same effect with the
    // standard implementation. But we want to flag unknown fields, and
    // #[serde(deny_unknown_fields)] is incompatible with #[serde(flatten)].
    let struct_name = &input.ident;
    let struct_generics = &input.generics.params;
    let struct_generic_names = input
        .generics
        .params
        .iter()
        .map(|param| match param {
            GenericParam::Lifetime(param) => param.lifetime.to_token_stream(),
            GenericParam::Type(param) => param.ident.to_token_stream(),
            GenericParam::Const(param) => param.ident.to_token_stream(),
        })
        .collect::<Vec<_>>();
    let struct_generic_types = input.generics.params.iter().filter_map(|param| {
        if let GenericParam::Type(param) = param {
            Some(&param.ident)
        } else {
            None
        }
    });

    let field_name = fields
        .named
        .iter()
        .map(|field| field.ident.as_ref())
        .collect::<Vec<_>>();
    let field_type = fields.named.iter().map(|field| &field.ty);

    quote! {
        impl<'de, #struct_generics> ::module_utils::serde::Deserialize<'de>
        for #struct_name<#(#struct_generic_names)*>
        where
            #struct_name<#(#struct_generic_names)*>: ::std::default::Default,
            #(#struct_generic_types: ::module_utils::serde::Deserialize<'de>,)*
        {
            fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
            where
                D: ::module_utils::serde::Deserializer<'de>
            {
                use ::module_utils::serde::de::Error;

                let map = #deserialize_map;

                #(
                    let deserializer = ::module_utils::serde::de::value::MapDeserializer::new(
                        map.clone().into_iter()
                    );
                    let #field_name = <#field_type>::deserialize(deserializer)
                        .map_err(D::Error::custom)?;
                )*
                Ok(Self {
                    #(#field_name,)*
                })
            }
        }
    }
}

pub(crate) fn merge_conf(attr: TokenStream, input: TokenStream) -> Result<TokenStream, Error> {
    let params: MergeConfParams = syn::parse(attr)?;
    let mut input: DeriveInput = syn::parse(input)?;

    // Derive Debug, Default and Deserialize implicitly
    let attributes = quote!(
        #[derive(::std::fmt::Debug, ::std::default::Default)]
    );
    let attributes = Attribute::parse_outer.parse2(attributes)?;
    input.attrs.extend(attributes);

    match &input.data {
        Data::Struct(struct_) => {
            if let Fields::Named(fields) = &struct_.fields {
                let implementation = generate_deserialize_impl(&input, fields, &params);

                Ok(quote! {
                    #input
                    #implementation
                }
                .into())
            } else {
                Err(Error::new_spanned(
                    &struct_.fields,
                    "merge_conf can only apply to named fields",
                ))
            }
        }
        Data::Enum(enum_) => Err(Error::new_spanned(
            enum_.enum_token,
            "merge_conf can only apply to struct",
        )),
        Data::Union(union_) => Err(Error::new_spanned(
            union_.union_token,
            "merge_conf can only apply to struct",
        )),
    }
}
