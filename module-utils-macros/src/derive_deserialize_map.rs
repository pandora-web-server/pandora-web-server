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
use serde_derive_internals::attr::RenameRule;
use syn::{spanned::Spanned, DeriveInput, Error, Field, FieldsNamed, Ident, LitStr, Path, Type};

use crate::utils::{generics_with_de, get_fields, type_name_short, where_clause};

#[derive(Clone)]
struct ContainerAttributes {
    rename_all: RenameRule,
    crate_path: Path,
}

impl TryFrom<&DeriveInput> for ContainerAttributes {
    type Error = Error;

    fn try_from(value: &DeriveInput) -> Result<Self, Self::Error> {
        let mut rename_all = RenameRule::None;
        let mut crate_path = None;

        for attr in &value.attrs {
            if !attr.path().is_ident("module_utils") {
                continue;
            }

            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename_all") {
                    if rename_all != RenameRule::None {
                        return Err(Error::new_spanned(meta.path, "duplicate rename_all"));
                    }
                    let mut lit = LitStr::new("", meta.path.span());
                    meta.parse_nested_meta(|meta| {
                        if meta.path.is_ident("deserialize") {
                            lit = meta.value()?.parse()?;
                            Ok(())
                        } else {
                            Err(Error::new_spanned(meta.path, "unexpected parameter"))
                        }
                    })
                    .or_else(|_| {
                        lit = meta.value()?.parse()?;
                        Ok::<_, Error>(())
                    })?;
                    rename_all = RenameRule::from_str(&lit.value())
                        .map_err(|_| Error::new_spanned(lit, "invalid rename_all value"))?;
                    Ok(())
                } else if meta.path.is_ident("crate") {
                    if crate_path.is_some() {
                        return Err(Error::new_spanned(meta.path, "duplicate crate"));
                    }
                    let lit: LitStr = meta.value()?.parse()?;
                    crate_path = Some(lit.parse()?);
                    Ok(())
                } else {
                    Err(Error::new_spanned(meta.path, "unexpected parameter"))
                }
            })?;
        }

        let crate_path = if let Some(crate_path) = crate_path {
            crate_path
        } else {
            syn::parse2(quote! {::module_utils})?
        };

        Ok(Self {
            rename_all,
            crate_path,
        })
    }
}

#[derive(Debug, Clone)]
struct FieldAttributes {
    skip: bool,
    name: Ident,
    ty: Type,
    deserialize_name: Vec<LitStr>,
    deserialize: TokenStream2,
    flatten: bool,
}

impl FieldAttributes {
    fn parse(field: &Field, container_attrs: &ContainerAttributes) -> Result<Self, Error> {
        let mut rename = None;
        let mut deserialize_name = Vec::new();
        let mut skip = false;
        let mut deserialize_with = None;
        let mut flatten = false;

        let name = if let Some(name) = &field.ident {
            name.clone()
        } else {
            skip = true;
            Ident::new("", Span::call_site())
        };

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
                            rename = Some(meta.value()?.parse()?);
                            Ok(())
                        } else {
                            Err(Error::new_spanned(meta.path, "unexpected parameter"))
                        }
                    })
                    .or_else(|_| {
                        rename = Some(meta.value()?.parse()?);
                        Ok::<_, Error>(())
                    })?;
                    Ok(())
                } else if meta.path.is_ident("alias") {
                    deserialize_name.push(meta.value()?.parse()?);
                    Ok(())
                } else if meta.path.is_ident("skip") || meta.path.is_ident("skip_deserializing") {
                    if skip {
                        return Err(Error::new_spanned(meta.path, "duplicate skip"));
                    }
                    skip = true;
                    Ok(())
                } else if meta.path.is_ident("flatten") {
                    flatten = true;
                    Ok(())
                } else if meta.path.is_ident("deserialize_with")
                    || meta.path.is_ident("deserialize_with_seed")
                    || meta.path.is_ident("with")
                {
                    if deserialize_with.is_some() {
                        return Err(Error::new_spanned(
                            meta.path,
                            "duplicate deserialization path",
                        ));
                    }
                    let value = meta.value()?;
                    let s: LitStr = value.parse()?;
                    let path = s.parse_with(Path::parse_mod_style)?;
                    deserialize_with = Some(if meta.path.is_ident("deserialize_with") {
                        quote! {#path(deserializer)}
                    } else if meta.path.is_ident("with") {
                        quote! {#path::deserialize(deserializer)}
                    } else {
                        quote! {#path(self.#name, deserializer)}
                    });
                    Ok(())
                } else {
                    Err(Error::new_spanned(meta.path, "unexpected parameter"))
                }
            })?;
        }

        if flatten {
            if let Some(rename) = rename {
                return Err(Error::new_spanned(
                    rename,
                    "rename is incompatible with flatten",
                ));
            }
            if let Some(deserialize_name) = deserialize_name.first() {
                return Err(Error::new_spanned(
                    deserialize_name,
                    "alias is incompatible with flatten",
                ));
            }
            if let Some(deserialize_with) = deserialize_with {
                return Err(Error::new_spanned(
                    deserialize_with,
                    "deserialize_with is incompatible with flatten",
                ));
            }
        }

        let ty = field.ty.clone();
        deserialize_name.insert(
            0,
            rename.unwrap_or_else(|| {
                let lit = container_attrs.rename_all.apply_to_field(&name.to_string());
                LitStr::new(lit.strip_prefix("r#").unwrap_or(&lit), name.span())
            }),
        );

        let crate_path = &container_attrs.crate_path;
        let deserialize = deserialize_with.unwrap_or_else(|| {
            quote! {
                {
                    use #crate_path::_private::DeserializeMerge;
                    (&&&&::std::marker::PhantomData::<#ty>).deserialize_merge(self.#name, deserializer)
                }
            }
        });

        Ok(Self {
            skip,
            name,
            ty,
            deserialize_name,
            deserialize,
            flatten,
        })
    }
}

fn collect_deserialize_names<'a>(attrs: &[&'a FieldAttributes]) -> Result<Vec<&'a LitStr>, Error> {
    let mut result = Vec::new();
    for attr in attrs {
        for name in &attr.deserialize_name {
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
    container_attrs: &ContainerAttributes,
) -> Result<TokenStream2, Error> {
    let vis = &input.vis;
    let struct_name = type_name_short(input);
    let (de, generics, generics_short) = generics_with_de(input);
    let crate_path = &container_attrs.crate_path;
    let where_clause = where_clause(input, fields, |field| {
        let attrs = FieldAttributes::parse(field, container_attrs).ok()?;
        if attrs.skip {
            None
        } else if attrs.flatten {
            Some(quote! {#crate_path::DeserializeMap<#de>})
        } else {
            Some(quote! {#crate_path::serde::Deserialize<#de>})
        }
    });

    let field_attrs = fields
        .named
        .iter()
        .map(|field| FieldAttributes::parse(field, container_attrs))
        .collect::<Result<Vec<_>, _>>()?;
    let field_name = field_attrs
        .iter()
        .map(|attr| &attr.name)
        .collect::<Vec<_>>();
    let inner_type = field_attrs
        .iter()
        .map(|attr| {
            let ty = &attr.ty;
            if attr.flatten {
                quote! {<#ty as #crate_path::DeserializeMap<#de>>::Visitor}
            } else {
                quote! {#ty}
            }
        })
        .collect::<Vec<_>>();
    let init = field_attrs.iter().map(|attr| {
        let field_name = &attr.name;
        if attr.flatten {
            quote! {self.#field_name.visitor()}
        } else {
            quote! {self.#field_name}
        }
    });
    let finalize = field_attrs.iter().map(|attr| {
        let field_name = &attr.name;
        if attr.flatten {
            quote! {self.#field_name.finalize()?}
        } else {
            quote! {self.#field_name}
        }
    });

    let flattened_name = field_attrs
        .iter()
        .filter(|attr| attr.flatten)
        .map(|attr| &attr.name);
    let flattened_type = field_attrs
        .iter()
        .zip(inner_type.iter())
        .filter_map(|(attr, ty)| if attr.flatten { Some(ty) } else { None })
        .collect::<Vec<_>>();

    let regular_fields = field_attrs
        .iter()
        .filter(|attr| !attr.skip && !attr.flatten)
        .collect::<Vec<_>>();
    let regular_name = regular_fields
        .iter()
        .map(|attr| &attr.name)
        .collect::<Vec<_>>();
    let regular_deserialize_name = regular_fields.iter().map(|attr| &attr.deserialize_name);
    let regular_deserialize = regular_fields.iter().map(|attr| &attr.deserialize);
    let deserialize_name = collect_deserialize_names(&regular_fields)?;

    Ok(quote! {
        const _: () = {
            const __FIELDS: &[&::std::primitive::str] = &[
                #(
                    #deserialize_name,
                )*
            ];

            #vis struct __Visitor<#generics> #where_clause {
                #(
                    #field_name: #inner_type,
                )*
                __marker: ::std::marker::PhantomData<&#de ()>,
            }

            impl<#generics> #crate_path::MapVisitor<#de> for __Visitor<#generics_short>
            #where_clause
            {
                type Value = #struct_name;

                fn accepts_field(field: &::std::primitive::str) -> ::std::primitive::bool {
                    if __FIELDS.contains(&field) {
                        return true;
                    }
                    #(
                        if #flattened_type::accepts_field(field) {
                            return true;
                        }
                    )*
                    false
                }

                fn list_fields(list: &mut ::std::vec::Vec<&'static ::std::primitive::str>) {
                    list.extend_from_slice(__FIELDS);
                    #(
                        #flattened_type::list_fields(list);
                    )*
                }

                fn visit_field<D>(mut self, field: &::std::primitive::str, deserializer: D)
                    -> ::std::result::Result<Self, D::Error>
                where
                    D: #crate_path::serde::de::Deserializer<#de>
                {
                    match field {
                        #(
                            #(#regular_deserialize_name)|* => {
                                self.#regular_name = #regular_deserialize?;
                                ::std::result::Result::Ok(self)
                            }
                        )*
                        other => {
                            #(
                                if #flattened_type::accepts_field(field) {
                                    self.#flattened_name = self.#flattened_name.visit_field(field, deserializer)?;
                                    return ::std::result::Result::Ok(self);
                                }
                            )*

                            let mut fields = ::std::vec::Vec::new();
                            Self::list_fields(&mut fields);
                            fields.sort();

                            // Error::unknown_field() won't accept non-static slices, so we
                            // duplicate its functionality here.
                            ::std::result::Result::Err(
                                <D::Error as #crate_path::serde::de::Error>::custom(
                                    ::std::format_args!(
                                        "unknown field `{field}`, expected one of `{}`",
                                        fields.join("`, `"),
                                    )
                                )
                            )
                        }
                    }
                }

                fn finalize<E>(self) -> Result<Self::Value, E>
                where
                    E: #crate_path::serde::de::Error
                {
                    ::std::result::Result::Ok(Self::Value {
                        #(
                            #field_name: #finalize,
                        )*
                    })
                }
            }

            impl<#generics> #crate_path::DeserializeMap<#de> for #struct_name
            #where_clause
            {
                type Visitor = __Visitor<#generics_short>;

                fn visitor(self) -> Self::Visitor {
                    Self::Visitor {
                        #(
                            #field_name: #init,
                        )*
                        __marker: ::std::marker::PhantomData,
                    }
                }
            }
        };
    })
}

fn generate_deserialize_impl(
    input: &DeriveInput,
    container_attrs: &ContainerAttributes,
) -> TokenStream2 {
    // This could be a blanket implementation for anything implementing DeserializeMap trait.
    // But it has to be an explicit implementation because blanket implementations for foreign
    // traits aren’t allowed.
    let struct_name = type_name_short(input);
    let (de, generics, generics_short) = generics_with_de(input);
    let crate_path = &container_attrs.crate_path;
    let mut where_clause = input
        .generics
        .where_clause
        .as_ref()
        .cloned()
        .unwrap_or_else(|| syn::parse2(quote! {where}).unwrap());
    where_clause.predicates.insert(
        0,
        syn::parse2(quote! {#struct_name: #crate_path::DeserializeMap<#de>}).unwrap(),
    );

    quote! {
        impl<#generics> #crate_path::serde::Deserialize<#de> for #struct_name #where_clause {
            fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
            where
                D: #crate_path::serde::Deserializer<#de>
            {
                use #crate_path::serde::de::DeserializeSeed;
                <Self as ::std::default::Default>::default().deserialize(deserializer)
            }
        }

        impl<#generics> #crate_path::serde::de::DeserializeSeed<#de> for #struct_name #where_clause {
            type Value = Self;

            fn deserialize<D>(self, deserializer: D) -> ::std::result::Result<Self::Value, D::Error>
            where
                D: #crate_path::serde::Deserializer<#de>
            {
                use #crate_path::{DeserializeMap, MapVisitor};

                struct __Visitor<#generics> #where_clause {
                    inner: <#struct_name as DeserializeMap<#de>>::Visitor,
                }

                impl<#generics> #crate_path::serde::de::Visitor<#de>
                for __Visitor<#generics_short> #where_clause
                {
                    type Value = #struct_name;

                    fn expecting(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                        formatter.write_str(::std::concat!("struct ", ::std::stringify!(#struct_name)))
                    }

                    fn visit_map<A>(mut self, mut map: A) -> ::std::result::Result<Self::Value, A::Error>
                    where
                        A: #crate_path::serde::de::MapAccess<#de>
                    {
                        struct __DeserializeSeed<T> {
                            key: ::std::string::String,
                            inner: T,
                        }

                        impl<#de, T> #crate_path::serde::de::DeserializeSeed<#de>
                        for __DeserializeSeed<T>
                        where
                            T: #crate_path::MapVisitor<#de>
                        {
                            type Value = T;

                            fn deserialize<D>(self, deserializer: D)
                                -> ::std::result::Result<Self::Value, D::Error>
                            where
                                D: #crate_path::serde::de::Deserializer<#de>
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
    let container_attrs = ContainerAttributes::try_from(&input)?;
    if let Some(fields) = get_fields(&input) {
        let deserialize_map = generate_deserialize_map_impl(&input, fields, &container_attrs)?;
        let deserialize = generate_deserialize_impl(&input, &container_attrs);
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
