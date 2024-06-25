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
use quote::{quote, ToTokens};
use syn::parse::Parser;
use syn::{Attribute, DeriveInput, Error};

use crate::utils::get_fields_mut;

pub(crate) fn merge_conf(input: TokenStream) -> Result<TokenStream, Error> {
    let mut input: DeriveInput = syn::parse(input)?;

    // Derive Debug, Default and DeserializeMap
    let attributes = quote! {
        #[derive(
            ::std::fmt::Debug,
            ::std::default::Default,
            ::pandora_module_utils::DeserializeMap
        )]
    };
    let attributes = Attribute::parse_outer.parse2(attributes)?;
    input.attrs.extend(attributes);

    if let Some(fields) = get_fields_mut(&mut input) {
        // Mark all fields as flattened
        for field in fields.named.iter_mut() {
            let attributes = quote! {#[pandora(flatten)]};
            let attributes = Attribute::parse_outer.parse2(attributes)?;
            field.attrs.extend(attributes)
        }

        Ok(input.into_token_stream().into())
    } else {
        Err(Error::new_spanned(
            &input,
            "merge_conf can only apply to structs with named fields",
        ))
    }
}
