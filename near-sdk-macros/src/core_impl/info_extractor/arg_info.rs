use crate::core_impl::info_extractor::{SerializerAttr, SerializerType};
use crate::core_impl::utils;
use proc_macro2::{Span, TokenStream};
use quote::ToTokens;
use syn::{spanned::Spanned, Attribute, Error, Ident, Pat, PatType, Token, Type};

pub enum BindgenArgType {
    /// Argument that we read from `env::input()`.
    Regular,
    /// An argument that we read from a single `env::promise_result()`.
    CallbackArg,
    /// An argument that we read from a single `env::promise_result()` which handles the error.
    CallbackResultArg,
    /// An argument that we read from all `env::promise_result()`.
    CallbackArgVec,
}

/// A single argument of a function after it was processed by the bindgen.
pub struct ArgInfo {
    /// Attributes not related to bindgen.
    pub non_bindgen_attrs: Vec<Attribute>,
    /// The `binding` part of `ref mut binding @ SUBPATTERN: TYPE` argument.
    pub ident: Ident,
    /// Whether pattern has a preceded `ref`.
    pub pat_reference: Option<Token![ref]>,
    /// Whether pattern has a preceded `mut`.
    pub pat_mutability: Option<Token![mut]>,
    /// Whether the `TYPE` starts with `&`.
    pub reference: Option<Token![&]>,
    /// Whether `TYPE` starts with `&mut`. Can only be set together with the `reference`.
    pub mutability: Option<Token![mut]>,
    /// The `TYPE` stripped of `&` and `mut`.
    pub ty: Type,
    /// Bindgen classification of argument type, based on what attributes it has.
    pub bindgen_ty: BindgenArgType,
    /// Type of serializer that we use for this argument.
    pub serializer_ty: SerializerType,
    /// Spans of all occurences of the `Self` token, if any.
    pub self_occurrences: Vec<Span>,
    /// The original `PatType` of the argument.
    pub original: PatType,
}

impl ArgInfo {
    /// Extract near-sdk specific argument info.
    pub fn new(original: &mut PatType, source_type: &TokenStream) -> syn::Result<Self> {
        let mut non_bindgen_attrs = vec![];
        let pat_info = match original.pat.as_ref() {
            Pat::Ident(pat_ident) => {
                Ok((pat_ident.by_ref, pat_ident.mutability, pat_ident.ident.clone()))
            }
            _ => Err(Error::new(
                original.span(),
                "Only identity patterns are supported in function arguments.",
            )),
        };

        let result_sanitize_and_ty = (|| {
            let sanitize_self = utils::sanitize_self(&original.ty, source_type)?;
            *original.ty.as_mut() = sanitize_self.ty.clone();
            let ty_info = utils::extract_ref_mut(original.ty.as_ref(), original.span())?;
            Ok((sanitize_self, ty_info))
        })();

        // In the absence of callback attributes this is a regular argument.
        let mut bindgen_ty = BindgenArgType::Regular;
        // In the absence of serialization attributes this is a JSON serialization.
        let mut serializer_ty = SerializerType::JSON;
        let mut more_errors: Vec<Error> = Vec::new();
        for attr in &original.attrs {
            let attr_str = attr.path.to_token_stream().to_string();
            match attr_str.as_str() {
                "callback" | "callback_unwrap" => {
                    bindgen_ty = BindgenArgType::CallbackArg;
                }
                "callback_result" => {
                    bindgen_ty = BindgenArgType::CallbackResultArg;
                }
                "callback_vec" => {
                    bindgen_ty = BindgenArgType::CallbackArgVec;
                }
                "serializer" => match syn::parse2::<SerializerAttr>(attr.tokens.clone()) {
                    Ok(serializer) => {
                        serializer_ty = serializer.serializer_type;
                    }
                    Err(e) => {
                        more_errors.push(e);
                    }
                },
                _ => {
                    non_bindgen_attrs.push((*attr).clone());
                }
            }
        }

        original.attrs.retain(|attr| {
            let attr_str = attr.path.to_token_stream().to_string();
            attr_str != "callback"
                && attr_str != "callback_vec"
                && attr_str != "serializer"
                && attr_str != "callback_result"
                && attr_str != "callback_unwrap"
        });

        match (&pat_info, &result_sanitize_and_ty, more_errors.is_empty()) {
            (
                Ok((pat_reference, pat_mutability, ident)),
                Ok((sanitize_self, (reference, mutability, ty))),
                true,
            ) => Ok(Self {
                non_bindgen_attrs,
                ident: ident.clone(),
                pat_reference: *pat_reference,
                pat_mutability: *pat_mutability,
                reference: *reference,
                mutability: *mutability,
                ty: ty.clone(),
                bindgen_ty,
                serializer_ty,
                self_occurrences: sanitize_self.self_occurrences.clone(),
                original: original.clone(),
            }),
            _ => {
                more_errors.extend(pat_info.err());
                more_errors.extend(result_sanitize_and_ty.err());
                Err(Self::combine_errors(more_errors).unwrap())
            }
        }
    }

    // helper function
    fn combine_errors(errors: impl IntoIterator<Item = Error>) -> Option<Error> {
        errors.into_iter().reduce(|mut acc, e| {
            acc.combine(syn::Error::new(e.span(), e.to_string()));
            acc
        })
    }
}
