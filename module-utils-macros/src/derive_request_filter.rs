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
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Error, Fields, Meta, Path};

pub(crate) fn derive_request_filter(input: TokenStream) -> Result<TokenStream, Error> {
    let mut input: DeriveInput = syn::parse(input)?;
    let mut merge_conf_tokens = Vec::new();
    input.attrs.retain(|attr| {
        fn our_path(path: &Path) -> bool {
            path.is_ident("merge_conf")
        }

        if let Meta::List(list) = &attr.meta {
            let our = our_path(&list.path);
            if our {
                merge_conf_tokens.push(list.tokens.clone());
            }
            !our
        } else if let Meta::Path(path) = &attr.meta {
            !our_path(path)
        } else {
            true
        }
    });

    match &input.data {
        Data::Struct(struct_) => {
            if let Fields::Named(fields) = &struct_.fields {
                let struct_name = &input.ident;

                // Produce merged handler configuration
                let mut conf = input.clone();
                conf.ident = format_ident!("{struct_name}Conf");
                let conf_name = &conf.ident;
                if let Data::Struct(struct_) = &mut conf.data {
                    if let Fields::Named(fields) = &mut struct_.fields {
                        for field in fields.named.iter_mut() {
                            let ty = &field.ty;
                            field.ty =
                                syn::parse2(quote!(<#ty as ::module_utils::RequestFilter>::Conf))?;
                        }
                    }
                }

                // Produce merged context
                let mut ctx = input.clone();
                ctx.ident = format_ident!("{struct_name}CTX");
                let ctx_name = &ctx.ident;
                if let Data::Struct(struct_) = &mut ctx.data {
                    if let Fields::Named(fields) = &mut struct_.fields {
                        for field in fields.named.iter_mut() {
                            let ty = &field.ty;
                            field.ty =
                                syn::parse2(quote!(<#ty as ::module_utils::RequestFilter>::CTX))?;
                        }
                    }
                }

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
                        #[::module_utils::merge_conf(#(#merge_conf_tokens,)*)]
                        #conf

                        #ctx

                        impl ::std::convert::TryFrom<#conf_name> for #struct_name {
                            type Error = ::std::boxed::Box<::module_utils::pingora::Error>;

                            fn try_from(conf: #conf_name) -> ::std::result::Result<Self, Self::Error> {
                                #(
                                    let #field_name = <#field_type>::try_from(conf.#field_name)?;
                                )*
                                ::std::result::Result::Ok(Self {
                                    #( #field_name, )*
                                })
                            }
                        }

                        #[async_trait::async_trait]
                        impl ::module_utils::RequestFilter for #struct_name {
                            type Conf = #conf_name;
                            type CTX = #ctx_name;

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
                        }
                    };
                }
                .into())
            } else {
                Err(Error::new_spanned(
                    &struct_.fields,
                    "RequestFilter can only be derived for named fields",
                ))
            }
        }
        Data::Enum(enum_) => Err(Error::new_spanned(
            enum_.enum_token,
            "RequestFilter can only be derived for struct",
        )),
        Data::Union(union_) => Err(Error::new_spanned(
            union_.union_token,
            "RequestFilter can only be derived for struct",
        )),
    }
}
