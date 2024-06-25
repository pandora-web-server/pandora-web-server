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

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::token::{Comma, Plus};
use syn::visit::Visit;
use syn::{
    Data, DataStruct, DeriveInput, Field, Fields, FieldsNamed, GenericParam, Ident, Lifetime,
    LifetimeParam, Type, TypeParamBound, WhereClause,
};

pub(crate) fn get_fields(ty: &DeriveInput) -> Option<&FieldsNamed> {
    if let Data::Struct(DataStruct {
        fields: Fields::Named(fields),
        ..
    }) = &ty.data
    {
        Some(fields)
    } else {
        None
    }
}

pub(crate) fn get_fields_mut(ty: &mut DeriveInput) -> Option<&mut FieldsNamed> {
    if let Data::Struct(DataStruct {
        fields: Fields::Named(fields),
        ..
    }) = &mut ty.data
    {
        Some(fields)
    } else {
        None
    }
}

fn generic_names(ty: &DeriveInput) -> (Vec<Lifetime>, Vec<Ident>) {
    ty.generics.params.iter().fold(
        (Vec::new(), Vec::new()),
        |(mut lifetimes, mut idents), param| {
            match param {
                GenericParam::Lifetime(param) => lifetimes.push(param.lifetime.clone()),
                GenericParam::Type(param) => idents.push(param.ident.clone()),
                GenericParam::Const(param) => idents.push(param.ident.clone()),
            };
            (lifetimes, idents)
        },
    )
}

fn contains_generic(ty: &Type, lifetimes: &[Lifetime], idents: &[Ident]) -> bool {
    struct Visitor<'a> {
        result: bool,
        lifetimes: &'a [Lifetime],
        idents: &'a [Ident],
    }

    impl<'ast> Visit<'ast> for Visitor<'_> {
        fn visit_ident(&mut self, ident: &'ast Ident) {
            if self.idents.contains(ident) {
                self.result = true;
            }
        }

        fn visit_lifetime(&mut self, lifetime: &'ast Lifetime) {
            if self.lifetimes.contains(lifetime) {
                self.result = true;
            }
        }
    }

    let mut visitor = Visitor {
        result: false,
        lifetimes,
        idents,
    };
    visitor.visit_type(ty);
    visitor.result
}

fn strip_generic_value(param: &GenericParam) -> GenericParam {
    match param {
        GenericParam::Lifetime(p) => GenericParam::Lifetime(LifetimeParam::new(p.lifetime.clone())),
        GenericParam::Type(p) => GenericParam::Type(p.ident.clone().into()),
        GenericParam::Const(p) => GenericParam::Type(p.ident.clone().into()),
    }
}

pub(crate) fn type_name_short(ty: &DeriveInput) -> TokenStream {
    let name = &ty.ident;
    let (lifetimes, idents) = generic_names(ty);
    quote! {
        #name <#(#lifetimes,)* #(#idents,)*>
    }
}

fn find_de(ty: &DeriveInput) -> Lifetime {
    let mut de = Lifetime::new("'de", Span::call_site());
    let mut i = 1;
    loop {
        if !ty
            .generics
            .params
            .iter()
            .any(|param| matches!(param, GenericParam::Lifetime(param) if param.lifetime == de))
        {
            return de;
        }
        i += 1;
        de = Lifetime::new(&format!("'de{i}"), Span::call_site());
    }
}

pub(crate) fn generics(
    ty: &DeriveInput,
) -> (
    Punctuated<GenericParam, Comma>,
    Punctuated<GenericParam, Comma>,
) {
    let generics = ty.generics.params.clone();
    let mut generics_short = generics.clone();
    for param in generics_short.iter_mut() {
        *param = strip_generic_value(param);
    }
    (generics, generics_short)
}

pub(crate) fn generics_with_de(
    ty: &DeriveInput,
) -> (
    Lifetime,
    Punctuated<GenericParam, Comma>,
    Punctuated<GenericParam, Comma>,
) {
    let de = find_de(ty);

    let mut generics = ty.generics.params.clone();
    generics.insert(0, GenericParam::Lifetime(LifetimeParam::new(de.clone())));

    let mut generics_short = generics.clone();
    for param in generics_short.iter_mut() {
        *param = strip_generic_value(param);
    }

    (de, generics, generics_short)
}

pub(crate) trait ToFieldBound<'a> {
    fn to_field_bound(&self, field: &'a Field) -> Option<Punctuated<TypeParamBound, Plus>>;
}
impl<'a> ToFieldBound<'a> for TokenStream {
    fn to_field_bound(&self, _field: &'a Field) -> Option<Punctuated<TypeParamBound, Plus>> {
        Some(
            Punctuated::<TypeParamBound, Plus>::parse_terminated
                .parse2(self.clone())
                .unwrap(),
        )
    }
}
impl<'a, F> ToFieldBound<'a> for F
where
    F: Fn(&'a Field) -> Option<TokenStream> + 'a,
{
    fn to_field_bound(&self, field: &'a Field) -> Option<Punctuated<TypeParamBound, Plus>> {
        self(field).map(|tokens| {
            Punctuated::<TypeParamBound, Plus>::parse_terminated
                .parse2(tokens)
                .unwrap()
        })
    }
}

pub(crate) fn where_clause<'a, B>(
    ty: &DeriveInput,
    fields: &'a FieldsNamed,
    field_bound: B,
) -> WhereClause
where
    B: ToFieldBound<'a>,
{
    let mut where_clause = ty
        .generics
        .where_clause
        .as_ref()
        .cloned()
        .unwrap_or_else(|| syn::parse2(quote! {where}).unwrap());

    let (lifetimes, idents) = ty.generics.params.iter().fold(
        (Vec::new(), Vec::new()),
        |(mut lifetimes, mut idents), param| {
            match param {
                GenericParam::Lifetime(param) => lifetimes.push(param.lifetime.clone()),
                GenericParam::Type(param) => idents.push(param.ident.clone()),
                GenericParam::Const(param) => idents.push(param.ident.clone()),
            };
            (lifetimes, idents)
        },
    );

    for field in &fields.named {
        let field_type = &field.ty;
        if contains_generic(field_type, &lifetimes, &idents) {
            if let Some(field_bound) = field_bound.to_field_bound(field) {
                where_clause
                    .predicates
                    .push(syn::parse2(quote! {#field_type: #field_bound}).unwrap());
            }
        }
    }

    where_clause
}
