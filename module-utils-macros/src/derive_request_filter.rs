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
use syn::{Data, DeriveInput, Error, Fields, Stmt};

pub(crate) fn derive_request_filter(input: TokenStream) -> Result<TokenStream, Error> {
    let input: DeriveInput = syn::parse(input)?;

    let mut conf = input.clone();
    let mut ctx = input.clone();
    let (try_from, request_filter) = match &input.data {
        Data::Struct(struct_) => {
            if let Fields::Named(fields) = &struct_.fields {
                // Produce merged handler configuration
                conf.ident = syn::parse2(quote!(__Conf))?;
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
                ctx.ident = syn::parse2(quote!(__CTX))?;
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
                let struct_name = &input.ident;
                let mut field_names = Vec::new();
                let mut conv_statements = Vec::new();
                let mut ctx_statements = Vec::new();
                for field in fields.named.iter() {
                    let name = if let Some(name) = &field.ident {
                        name
                    } else {
                        continue;
                    };
                    let ty = &field.ty;

                    field_names.push(name.clone());
                    conv_statements.push(syn::parse2::<Stmt>(quote! {
                        let #name = <#ty>::try_from(conf.#name)?;
                    })?);
                    ctx_statements.push(syn::parse2::<Stmt>(quote! {
                        let #name = <#ty>::new_ctx();
                    })?);
                }

                // Produce TryFrom implementation
                let try_from = quote!(
                    impl ::std::convert::TryFrom<__Conf> for #struct_name {
                        type Error = ::std::boxed::Box<::pingora_core::Error>;

                        fn try_from(conf: __Conf) -> ::std::result::Result<Self, Self::Error> {
                            #( #conv_statements )*
                            ::std::result::Result::Ok(Self {
                                #( #field_names, )*
                            })
                        }
                    }
                );

                // Produce RequestFilter implementation
                let request_filter = quote! {
                    #[async_trait::async_trait]
                    impl ::module_utils::RequestFilter for #struct_name {
                        type Conf = __Conf;
                        type CTX = __CTX;

                        fn new_ctx() -> Self::CTX {
                            #( #ctx_statements )*
                            Self::CTX {
                                #( #field_names, )*
                            }
                        }

                        async fn request_filter(
                            &self,
                            _session: &mut ::pingora_proxy::Session,
                            _ctx: &mut Self::CTX,
                        ) -> ::std::result::Result<::module_utils::RequestFilterResult, ::std::boxed::Box<::pingora_core::Error>> {
                            #(
                                let result = self.#field_names.request_filter(_session, &mut _ctx.#field_names).await?;
                                if result != ::module_utils::RequestFilterResult::Unhandled {
                                    return ::std::result::Result::Ok(result);
                                }
                            )*
                            ::std::result::Result::Ok(module_utils::RequestFilterResult::Unhandled)
                        }
                    }
                };

                (try_from, request_filter)
            } else {
                return Err(Error::new_spanned(
                    &struct_.fields,
                    "RequestFilter can only be derived for named fields",
                ));
            }
        }
        Data::Enum(enum_) => {
            return Err(Error::new_spanned(
                enum_.enum_token,
                "RequestFilter can only be derived for struct",
            ));
        }
        Data::Union(union_) => {
            return Err(Error::new_spanned(
                union_.union_token,
                "RequestFilter can only be derived for struct",
            ));
        }
    };

    Ok(quote! {
        #[::module_utils::merge_conf]
        #conf

        #ctx

        #try_from

        #request_filter
    }
    .into())
}
