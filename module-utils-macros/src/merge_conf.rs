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
use quote::quote;
use syn::parse::Parser;
use syn::{Attribute, DeriveInput, Error, FieldsNamed};

use crate::derive_deserialize_map::generate_deserialize_impl;
use crate::utils::{generics_with_de, get_fields, type_name_short, where_clause};

fn generate_deserialize_map_impl(input: &DeriveInput, fields: &FieldsNamed) -> TokenStream2 {
    let vis = &input.vis;
    let struct_name = type_name_short(input);
    let (de, generics, generics_short) = generics_with_de(input);
    let where_clause = where_clause(input, fields, quote! {::module_utils::DeserializeMap<#de>});

    let field_name = fields
        .named
        .iter()
        .map(|field| field.ident.as_ref())
        .collect::<Vec<_>>();
    let field_visitor = fields
        .named
        .iter()
        .map(|field| &field.ty)
        .map(|ty| quote! {<#ty as ::module_utils::DeserializeMap<#de>>::Visitor})
        .collect::<Vec<_>>();
    quote! {
        const _: () = {
            #vis struct __Visitor<#generics> #where_clause {
                #(
                    #field_name: #field_visitor,
                )*
            }

            impl<#generics> ::module_utils::MapVisitor<#de> for __Visitor<#generics_short> #where_clause {
                type Value = #struct_name;

                fn accepts_field(field: &::std::primitive::str) -> ::std::primitive::bool {
                    #(
                        if #field_visitor::accepts_field(field) {
                            return true;
                        }
                    )*
                    false
                }

                fn list_fields(list: &mut ::std::vec::Vec<&'static ::std::primitive::str>) {
                    #(
                        #field_visitor::list_fields(list);
                    )*
                }

                fn visit_field<D>(
                    &mut self,
                    field: &::std::primitive::str,
                    deserializer: D
                ) -> ::std::result::Result<(), D::Error>
                where
                    D: ::module_utils::serde::de::Deserializer<#de>
                {
                    #(
                        if #field_visitor::accepts_field(field) {
                            return self.#field_name.visit_field(field, deserializer);
                        }
                    )*

                    let mut fields = ::std::vec::Vec::new();
                    Self::list_fields(&mut fields);
                    fields.sort();

                    // Error::unknown_field() won't accept non-static slices, so we
                    // duplicate its functionality here.
                    return ::std::result::Result::Err(
                        <D::Error as ::module_utils::serde::de::Error>::custom(
                            ::std::format_args!(
                                "unknown field `{field}`, expected one of `{}`",
                                fields.join("`, `"),
                            )
                        )
                    );
                }

                fn finalize<E>(self) -> Result<Self::Value, E>
                where
                    E: ::module_utils::serde::de::Error
                {
                    ::std::result::Result::Ok(Self::Value {
                        #(
                            #field_name: self.#field_name.finalize()?,
                        )*
                    })
                }
            }

            impl<#generics> ::module_utils::DeserializeMap<#de> for #struct_name #where_clause
            {
                type Visitor = __Visitor<#generics_short>;

                fn visitor(self) -> Self::Visitor {
                    Self::Visitor {
                        #(
                            #field_name: self.#field_name.visitor(),
                        )*
                    }
                }
            }
        };
    }
}

pub(crate) fn merge_conf(input: TokenStream) -> Result<TokenStream, Error> {
    let mut input: DeriveInput = syn::parse(input)?;

    // Derive Debug and Default implicitly
    let attributes = quote! {
        #[derive(::std::fmt::Debug, ::std::default::Default)]
    };
    let attributes = Attribute::parse_outer.parse2(attributes)?;
    input.attrs.extend(attributes);

    if let Some(fields) = get_fields(&input) {
        let implementation_map = generate_deserialize_map_impl(&input, fields);
        let implementation_deserialize = generate_deserialize_impl(&input);

        Ok(quote! {
            #input
            #implementation_map
            #implementation_deserialize
        }
        .into())
    } else {
        Err(Error::new_spanned(
            &input,
            "merge_conf can only apply to structs with named fields",
        ))
    }
}
