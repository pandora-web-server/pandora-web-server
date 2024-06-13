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
use syn::{DeriveInput, Error, FieldsNamed, Ident};

use crate::utils::{generics, get_fields, get_fields_mut, type_name_short, where_clause};

fn generate_request_filter_impl(
    input: &DeriveInput,
    fields: &FieldsNamed,
) -> Result<TokenStream, Error> {
    let struct_name = type_name_short(input);
    let (generics, generics_short) = generics(input);
    let where_clause = where_clause(input, fields, quote! {::std::marker::Sync});

    // Produce merged handler configuration
    let mut conf = input.clone();
    conf.ident = Ident::new("__Conf", input.ident.span());
    if let Some(fields) = get_fields_mut(&mut conf) {
        for field in fields.named.iter_mut() {
            let ty = &field.ty;
            field.ty = syn::parse2(quote! {<#ty as ::module_utils::RequestFilter>::Conf})?;
        }
    }
    let conf_name = &conf.ident;

    // Produce merged context
    let mut ctx = input.clone();
    ctx.ident = Ident::new("__CTX", input.ident.span());
    if let Some(fields) = get_fields_mut(&mut ctx) {
        for field in fields.named.iter_mut() {
            let ty = &field.ty;
            field.ty = syn::parse2(quote! {<#ty as ::module_utils::RequestFilter>::CTX})?;
        }
    }
    let ctx_name = &ctx.ident;

    // Collect field data
    let field_name = fields
        .named
        .iter()
        .map(|field| field.ident.as_ref())
        .collect::<Vec<_>>();
    let field_type = fields
        .named
        .iter()
        .map(|field| &field.ty)
        .collect::<Vec<_>>();

    Ok(quote! {
        const _: () = {
            #[::module_utils::merge_conf]
            #conf

            #ctx

            impl<#generics> ::std::convert::TryFrom<#conf_name<#generics_short>>
            for #struct_name #where_clause
            {
                type Error = ::std::boxed::Box<::module_utils::pingora::Error>;

                fn try_from(conf: #conf_name<#generics_short>)
                    -> ::std::result::Result<Self, Self::Error>
                {
                    #(
                        let #field_name = <#field_type>::try_from(conf.#field_name)?;
                    )*
                    ::std::result::Result::Ok(Self {
                        #( #field_name, )*
                    })
                }
            }

            #[::module_utils::async_trait::async_trait]
            impl<#generics> ::module_utils::RequestFilter for #struct_name
            #where_clause
            {
                type Conf = #conf_name<#generics_short>;
                type CTX = #ctx_name<#generics_short>;

                fn new_ctx() -> Self::CTX {
                    #(
                        let #field_name = <#field_type>::new_ctx();
                    )*
                    Self::CTX {
                        #( #field_name, )*
                    }
                }

                async fn request_filter(
                    &self,
                    _session: &mut impl ::module_utils::pingora::SessionWrapper,
                    _ctx: &mut Self::CTX,
                ) -> ::std::result::Result<::module_utils::RequestFilterResult, ::std::boxed::Box<::module_utils::pingora::Error>> {
                    #(
                        let result = self.#field_name.request_filter(_session, &mut _ctx.#field_name).await?;
                        if result != ::module_utils::RequestFilterResult::Unhandled {
                            return ::std::result::Result::Ok(result);
                        }
                    )*
                    ::std::result::Result::Ok(module_utils::RequestFilterResult::Unhandled)
                }

                fn request_filter_done(
                    &self,
                    _session: &mut impl ::module_utils::pingora::SessionWrapper,
                    _ctx: &mut Self::CTX,
                    _result: ::module_utils::RequestFilterResult,
                ) {
                    #(
                        self.#field_name.request_filter_done(_session, &mut _ctx.#field_name, _result);
                    )*
                }

                async fn upstream_peer(
                    &self,
                    _session: &mut impl ::module_utils::pingora::SessionWrapper,
                    _ctx: &mut Self::CTX,
                ) -> ::std::result::Result<
                    ::std::option::Option<::std::boxed::Box<::module_utils::pingora::HttpPeer>>,
                    ::std::boxed::Box<::module_utils::pingora::Error>
                >
                {
                    #(
                        if let Some(peer) =
                            self.#field_name.upstream_peer(_session, &mut _ctx.#field_name).await?
                        {
                            return Ok(Some(peer));
                        }
                    )*
                    Ok(None)
                }

                fn response_filter(
                    &self,
                    _session: &mut impl ::module_utils::pingora::SessionWrapper,
                    _response: &mut ::module_utils::pingora::ResponseHeader,
                    mut _ctx: ::std::option::Option<&mut Self::CTX>,
                ) {
                    #(
                        self.#field_name.response_filter(_session, _response, _ctx.as_mut().map(|ctx| &mut ctx.#field_name));
                    )*
                }

                async fn logging(
                    &self,
                    _session: &mut impl ::module_utils::pingora::SessionWrapper,
                    _e: ::std::option::Option<&::module_utils::pingora::Error>,
                    _ctx: &mut Self::CTX,
                ) {
                    #(
                        self.#field_name.logging(_session, _e, &mut _ctx.#field_name).await;
                    )*
                }
            }
        };
    }
    .into())
}

pub(crate) fn derive_request_filter(input: TokenStream) -> Result<TokenStream, Error> {
    let input: DeriveInput = syn::parse(input)?;
    if let Some(fields) = get_fields(&input) {
        generate_request_filter_impl(&input, fields)
    } else {
        Err(Error::new_spanned(
            &input,
            "RequestFilter can only be derived for structs with named fields",
        ))
    }
}
