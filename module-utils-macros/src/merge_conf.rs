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
use quote::quote;
use syn::{parse::Parser, Attribute, Data, DeriveInput, Error, Fields};

pub(crate) fn merge_conf(input: TokenStream) -> Result<TokenStream, Error> {
    let mut input: DeriveInput = syn::parse(input)?;

    // Derive Debug, Default and Deserialize implicitly
    let attributes = quote!(
        #[derive(::std::fmt::Debug, ::std::default::Default, ::serde::Deserialize)]
        #[serde(default)]
    );
    let attributes = Attribute::parse_outer.parse2(attributes)?;
    input.attrs.extend(attributes);

    match &mut input.data {
        Data::Struct(struct_) => {
            if let Fields::Named(fields) = &mut struct_.fields {
                // Make serde flatten all fields
                for field in fields.named.iter_mut() {
                    let attributes = quote!(#[serde(flatten)]);
                    let attributes = Attribute::parse_outer.parse2(attributes)?;
                    field.attrs.extend(attributes)
                }
            } else {
                return Err(Error::new_spanned(
                    &struct_.fields,
                    "merge_conf can only apply to named fields",
                ));
            }
        }
        Data::Enum(enum_) => {
            return Err(Error::new_spanned(
                enum_.enum_token,
                "merge_conf can only apply to struct",
            ));
        }
        Data::Union(union_) => {
            return Err(Error::new_spanned(
                union_.union_token,
                "merge_conf can only apply to struct",
            ));
        }
    }

    Ok(quote! {#input}.into())
}
