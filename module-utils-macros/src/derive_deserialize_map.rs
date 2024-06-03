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
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{parse::Parse, DeriveInput, Error, Field, FieldsNamed, Ident, LitStr, Path};

use crate::utils::{generics_with_de, get_fields, type_name_short, where_clause};

#[derive(Debug)]
struct FieldAttributes {
    skip: bool,
    name: Ident,
    deserialize_from: Vec<Ident>,
    deserialize: TokenStream2,
}

impl TryFrom<&Field> for FieldAttributes {
    type Error = Error;

    fn try_from(field: &Field) -> Result<Self, Self::Error> {
        let mut rename = None;
        let mut deserialize_from = Vec::new();
        let mut skip = false;
        let mut deserialize_with = None;

        for attr in &field.attrs {
            if !attr.path().is_ident("module_utils") {
                continue;
            }

            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename") {
                    if rename.is_some() {
                        return Err(Error::new_spanned(meta.path, "duplicate rename"));
                    }
                    meta.parse_nested_meta(|meta| {
                        if meta.path.is_ident("deserialize") {
                            let value = meta.value()?;
                            let s: LitStr = value.parse()?;
                            rename = Some(s.parse_with(Ident::parse)?);
                            Ok(())
                        } else {
                            Err(Error::new_spanned(meta.path, "unexpected parameter"))
                        }
                    })
                    .or_else(|_| {
                        let value = meta.value()?;
                        let s: LitStr = value.parse()?;
                        rename = Some(s.parse_with(Ident::parse)?);
                        Ok::<_, Error>(())
                    })?;
                    Ok(())
                } else if meta.path.is_ident("alias") {
                    let value = meta.value()?;
                    let s: LitStr = value.parse()?;
                    deserialize_from.push(s.parse_with(Ident::parse)?);
                    Ok(())
                } else if meta.path.is_ident("skip") || meta.path.is_ident("skip_deserializing") {
                    if skip {
                        return Err(Error::new_spanned(meta.path, "duplicate skip"));
                    }
                    skip = true;
                    Ok(())
                } else if meta.path.is_ident("deserialize_with") || meta.path.is_ident("with") {
                    if deserialize_with.is_some() {
                        return Err(Error::new_spanned(
                            meta.path,
                            "duplicate deserialization path",
                        ));
                    }
                    let value = meta.value()?;
                    let s: LitStr = value.parse()?;
                    let mut path = s.parse_with(Path::parse_mod_style)?;
                    if meta.path.is_ident("with") {
                        path.segments
                            .push(Ident::new("deserialize", s.span()).into());
                    }
                    deserialize_with = Some(path);
                    Ok(())
                } else {
                    Err(Error::new_spanned(meta.path, "unexpected parameter"))
                }
            })?;
        }

        let name = if let Some(name) = &field.ident {
            name.clone()
        } else {
            skip = true;
            Ident::new("", Span::call_site())
        };
        deserialize_from.insert(0, rename.unwrap_or_else(|| name.clone()));

        let deserialize = if let Some(deserialize_with) = deserialize_with {
            quote! {#deserialize_with(deserializer)}
        } else {
            let field_name = &field.ident;
            let field_type = &field.ty;
            quote! {
                {
                    use ::module_utils::_private::DeserializeMerge;
                    (&&&&::std::marker::PhantomData::<#field_type>).deserialize_merge(self.inner.#field_name, deserializer)
                }
            }
        };

        Ok(Self {
            skip,
            name,
            deserialize_from,
            deserialize,
        })
    }
}

fn collect_deserialize_names(attrs: &[FieldAttributes]) -> Result<Vec<&Ident>, Error> {
    let mut result = Vec::new();
    for attr in attrs {
        for name in &attr.deserialize_from {
            if result.contains(&name) {
                return Err(Error::new(name.span(), "duplicate field name"));
            }
            result.push(name);
        }
    }
    Ok(result)
}

fn generate_deserialize_map_impl(
    input: &DeriveInput,
    fields: &FieldsNamed,
) -> Result<TokenStream2, Error> {
    let vis = &input.vis;
    let struct_name = type_name_short(input);
    let (de, generics, generics_short) = generics_with_de(input);
    let where_clause = where_clause(
        input,
        fields,
        quote! {::module_utils::serde::Deserialize<#de>},
    );

    let mut field_attrs = fields
        .named
        .iter()
        .map(FieldAttributes::try_from)
        .collect::<Result<Vec<_>, _>>()?;
    field_attrs.retain(|attr| !attr.skip);

    let field_name = field_attrs.iter().map(|attr| &attr.name);
    let field_deserialize_name = field_attrs.iter().map(|attr| &attr.deserialize_from);
    let field_deserialize = field_attrs.iter().map(|attr| &attr.deserialize);
    let deserialize_name = collect_deserialize_names(&field_attrs)?;

    Ok(quote! {
        const _: () = {
            const __FIELDS: &[&::std::primitive::str] = &[
                #(
                    ::std::stringify!(#deserialize_name),
                )*
            ];

            #vis struct __Visitor<#generics> #where_clause {
                inner: #struct_name,
                marker: ::std::marker::PhantomData<&#de ()>,
            }

            impl<#generics> ::module_utils::MapVisitor<#de> for __Visitor<#generics_short>
            #where_clause
            {
                type Value = #struct_name;

                fn accepts_field(field: &::std::primitive::str) -> ::std::primitive::bool {
                    __FIELDS.contains(&field)
                }

                fn list_fields(list: &mut ::std::vec::Vec<&'static ::std::primitive::str>) {
                    list.extend_from_slice(__FIELDS);
                }

                fn visit_field<D>(mut self, field: &::std::primitive::str, deserializer: D)
                    -> ::std::result::Result<Self, D::Error>
                where
                    D: ::module_utils::serde::de::Deserializer<#de>
                {
                    match field {
                        #(
                            #(::std::stringify!(#field_deserialize_name))|* => {
                                self.inner.#field_name = #field_deserialize?;
                                ::std::result::Result::Ok(self)
                            }
                        )*
                        other => ::std::result::Result::Err(
                            <D::Error as ::module_utils::serde::de::Error>::unknown_field(
                                other,
                                __FIELDS
                            )
                        ),
                    }
                }

                fn finalize<E>(self) -> Result<Self::Value, E>
                where
                    E: ::module_utils::serde::de::Error
                {
                    ::std::result::Result::Ok(self.inner)
                }
            }

            impl<#generics> ::module_utils::DeserializeMap<#de> for #struct_name
            #where_clause
            {
                type Visitor = __Visitor<#generics_short>;

                fn visitor(self) -> Self::Visitor {
                    Self::Visitor {
                        inner: self,
                        marker: ::std::marker::PhantomData,
                    }
                }
            }
        };
    })
}

pub(crate) fn generate_deserialize_impl(input: &DeriveInput) -> TokenStream2 {
    // This could be a blanket implementation for anything implementing DeserializeMap trait.
    // But it has to be an explicit implementation because blanket implementations for foreign
    // traits aren’t allowed.
    let struct_name = type_name_short(input);
    let (de, generics, generics_short) = generics_with_de(input);
    let mut where_clause = input
        .generics
        .where_clause
        .as_ref()
        .cloned()
        .unwrap_or_else(|| syn::parse2(quote! {where}).unwrap());
    where_clause.predicates.insert(
        0,
        syn::parse2(quote! {#struct_name: ::module_utils::DeserializeMap<#de>}).unwrap(),
    );

    quote! {
        impl<#generics> ::module_utils::serde::Deserialize<#de> for #struct_name #where_clause {
            fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
            where
                D: ::module_utils::serde::Deserializer<#de>
            {
                use ::module_utils::serde::de::DeserializeSeed;
                <Self as ::std::default::Default>::default().deserialize(deserializer)
            }
        }

        impl<#generics> ::module_utils::serde::de::DeserializeSeed<#de> for #struct_name #where_clause {
            type Value = Self;

            fn deserialize<D>(self, deserializer: D) -> ::std::result::Result<Self::Value, D::Error>
            where
                D: ::module_utils::serde::Deserializer<#de>
            {
                use ::module_utils::{DeserializeMap, MapVisitor};

                struct __Visitor<#generics> #where_clause {
                    inner: <#struct_name as DeserializeMap<#de>>::Visitor,
                }

                impl<#generics> ::module_utils::serde::de::Visitor<#de>
                for __Visitor<#generics_short> #where_clause
                {
                    type Value = #struct_name;

                    fn expecting(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                        formatter.write_str(::std::concat!("struct ", ::std::stringify!(#struct_name)))
                    }

                    fn visit_map<A>(mut self, mut map: A) -> ::std::result::Result<Self::Value, A::Error>
                    where
                        A: ::module_utils::serde::de::MapAccess<#de>
                    {
                        struct __DeserializeSeed<T> {
                            key: ::std::string::String,
                            inner: T,
                        }

                        impl<#de, T> ::module_utils::serde::de::DeserializeSeed<#de>
                        for __DeserializeSeed<T>
                        where
                            T: ::module_utils::MapVisitor<#de>
                        {
                            type Value = T;

                            fn deserialize<D>(self, deserializer: D)
                                -> ::std::result::Result<Self::Value, D::Error>
                            where
                                D: ::module_utils::serde::de::Deserializer<#de>
                            {
                                self.inner.visit_field(&self.key, deserializer)
                            }
                        }

                        while let ::std::option::Option::Some(key) =
                            map.next_key::<::std::string::String>()?
                        {
                            self.inner = map.next_value_seed(__DeserializeSeed {
                                key,
                                inner: self.inner,
                            })?;
                        }
                        self.inner.finalize()
                    }
                }

                let visitor = __Visitor {
                    inner: self.visitor(),
                };
                deserializer.deserialize_map(visitor)
            }
        }
    }
}

pub(crate) fn derive_deserialize_map(input: TokenStream) -> Result<TokenStream, Error> {
    let input: DeriveInput = syn::parse(input)?;
    if let Some(fields) = get_fields(&input) {
        let deserialize_map = generate_deserialize_map_impl(&input, fields)?;
        let deserialize = generate_deserialize_impl(&input);
        Ok(quote! {
            #deserialize_map
            #deserialize
        }
        .into())
    } else {
        Err(Error::new_spanned(
            &input,
            "DeserializeMap can only be derived for structs with named fields",
        ))
    }
}
