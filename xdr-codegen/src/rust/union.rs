//! Unions, as `#[repr(i32)]` Rust enums.
//!
//! Each `case` label becomes a variant, whose Rust discriminant (its tag) is
//! the case value as an `i32`; an unsigned value keeps its bit pattern. The
//! variant is named after the label: an enum member's name, `TRUE` or
//! `FALSE`, or `_` followed by a number (`_5`, `_neg1`, `_0x10`) or a
//! constant's name (`_kFoo`). An arm with several labels becomes one
//! variant per label. The default arm becomes a `Default` variant holding
//! the discriminant, whose tag is one that no case uses.
//!
//! The derives in `xdrs` read the switch type from
//! `#[xdr(discriminant = "...")]` and encode the tag accordingly.

use quote::quote;

use super::Gen;
use super::comment::Comment;
use crate::ast::*;
use crate::diag::Span;

/// What a `case` label is.
enum Case<'a> {
    /// A member of the enum that switches the union.
    Member(&'a Ident),
    /// `TRUE` or `FALSE`.
    Bool(bool),
    /// A number.
    Number(&'a Number),
    /// A constant defined in the file.
    Const(&'a Ident),
}

impl Gen<'_> {
    pub(super) fn union_def(
        &mut self,
        name: &str,
        span: Span,
        body: &UnionBody,
        comment: &Comment,
    ) {
        let markers = comment.markers;
        let rust_name = self.ident(name, span);
        let (switch, switch_named) = match &body.switch_type {
            TypeSpec::Int => (quote! { i32 }, false),
            TypeSpec::UnsignedInt => (quote! { u32 }, false),
            TypeSpec::Bool => (quote! { bool }, false),
            TypeSpec::Named(i) => {
                let i = self.name(i);
                (quote! { #i }, true)
            }
            _ => {
                self.error(
                    body.switch_name.span,
                    "a union can switch only on `int`, `unsigned int`, `bool` or an enum",
                );
                return;
            }
        };
        let is_bool = body.switch_type == TypeSpec::Bool;
        let is_i32 = body.switch_type == TypeSpec::Int;
        let switch_id = self.name(&body.switch_name);

        let mut variants = Vec::new();
        let mut accessor = Vec::new();
        let mut tags = Vec::new();
        let mut default = None;
        for arm in &body.arms {
            let arm_comment = Comment::parse(&arm.member.comment);
            let payload = match &arm.member.decl {
                Some(decl) => {
                    let inline_name = format!("{name}_{}", decl.name.name);
                    let ty = self.decl_type(
                        &decl.ty,
                        &inline_name,
                        arm.member.span,
                        arm_comment.markers.box_ptr,
                        markers,
                    );
                    let field = self.name(&decl.name);
                    let hex = self.opaque_attrs(&decl.ty);
                    Some(quote! { #hex #field: #ty })
                }
                None => None,
            };
            let mut doc = Some(arm_comment.attrs());
            for value in &arm.cases {
                let Some(case) = self.case(value, &body.switch_type, switch_named) else {
                    continue;
                };
                self.check_case_range(&case, value, &body.switch_type);
                let variant = self.ident(&variant_name(&case), value.span());
                let case_value = match &case {
                    Case::Member(m) => {
                        let m = self.name(m);
                        quote! { #switch::#m }
                    }
                    Case::Bool(b) if is_bool => quote! { #b },
                    Case::Bool(b) => quote! { #b as #switch },
                    Case::Number(n) => {
                        let n = self.int(n);
                        quote! { #n as #switch }
                    }
                    Case::Const(c) => {
                        let c = self.name(c);
                        quote! { #c as #switch }
                    }
                };
                let tag = match &case {
                    Case::Member(_) => quote! { #case_value.0 as i32 },
                    _ if is_i32 => case_value.clone(),
                    _ => quote! { #case_value as i32 },
                };
                let doc = doc.take();
                let (fields, pattern) = match &payload {
                    Some(p) => (quote! { { #p } }, quote! { { .. } }),
                    None => (quote! {}, quote! {}),
                };
                variants.push(quote! { #doc #variant #fields = #tag, });
                accessor.push(quote! { #rust_name::#variant #pattern => #case_value, });
                tags.push(tag);
            }
            if arm.is_default {
                default = Some((doc.take(), payload));
            }
        }

        if let Some((doc, payload)) = default {
            // Both the encoding derives and `derive(XDR)` read it.
            let attr = self.attr_any(&["xdrs", "spec"], quote! { xdr(default_variant) });
            let payload = payload.map(|p| quote! { , #p });
            // The derived const writers write this field through its type's
            // own const methods, or directly if the type is named as a
            // primitive, which an alias of one is not.
            let field_ty = match self.primitive(&body.switch_type) {
                Some(TypeSpec::Int) => quote! { i32 },
                Some(TypeSpec::UnsignedInt) => quote! { u32 },
                Some(TypeSpec::Bool) => quote! { bool },
                _ => switch.clone(),
            };
            // The tag never goes on the wire, as the variant holds the
            // discriminant, but it must differ from every case's. The cases
            // may be enum members or constants from other files, so rustc
            // finds the tag: the least i32 that no case uses.
            variants.push(quote! {
                #doc
                #attr
                Default { #switch_id: #field_ty #payload } = {
                    const __XDR_CASES: &[i32] = &[#(#tags),*];
                    let mut tag = i32::MIN;
                    let mut i = 0;
                    while i < __XDR_CASES.len() {
                        if __XDR_CASES[i] == tag {
                            tag += 1;
                            i = 0;
                        } else {
                            i += 1;
                        }
                    }
                    tag
                },
            });
            accessor.push(quote! { #rust_name::Default { #switch_id, .. } => *#switch_id, });
        }

        let doc = comment.attrs();
        let std_derives = Self::std_derives(markers);
        let derives = self.derives(markers);
        let discriminant = switch.to_string();
        let discriminant = self.attr("xdrs", quote! { xdr(discriminant = #discriminant) });
        let schema = self.schema_derive();
        self.items.push(quote! {
            #doc
            #std_derives
            #derives
            #discriminant
            #schema
            #[repr(i32)]
            #[non_exhaustive]
            pub enum #rust_name {
                #(#variants)*
            }
        });
        self.items.push(quote! {
            impl #rust_name {
                /// The discriminant.
                #[allow(clippy::match_single_binding)]
                pub fn #switch_id(&self) -> #switch {
                    match self {
                        #(#accessor)*
                    }
                }
            }
        });
    }

    /// The primitive that `ty` is, following the file's type aliases:
    /// `int`, `unsigned int` or `bool`, or `None` for any other type,
    /// including a name defined in another file.
    fn primitive(&self, ty: &TypeSpec) -> Option<TypeSpec> {
        let mut ty = ty;
        // Bounded, in case the aliases form a cycle.
        for _ in 0..=self.aliases.len() {
            match ty {
                TypeSpec::Int | TypeSpec::UnsignedInt | TypeSpec::Bool => return Some(ty.clone()),
                TypeSpec::Named(i) => ty = self.aliases.get(&i.name)?,
                _ => return None,
            }
        }
        None
    }

    /// Reports a case value that does not fit the switch type, when both
    /// are known: a number or a constant of this file, and a switch that
    /// is `int` or `unsigned int`, directly or through aliases. Rust would
    /// reject a literal that does not fit, but truncate a constant.
    fn check_case_range(&mut self, case: &Case<'_>, value: &Value, switch: &TypeSpec) {
        let v = match case {
            Case::Number(n) => n.value,
            Case::Const(c) => match self.consts.get(&c.name) {
                Some(&v) => v,
                None => return,
            },
            _ => return,
        };
        let (range, ty) = match self.primitive(switch) {
            Some(TypeSpec::Int) => (i128::from(i32::MIN)..=i128::from(i32::MAX), "int"),
            Some(TypeSpec::UnsignedInt) => (0..=i128::from(u32::MAX), "unsigned int"),
            _ => return,
        };
        if !range.contains(&v) {
            let shown = match case {
                Case::Const(c) => format!("`{}` ({v})", c.name),
                _ => format!("`{v}`"),
            };
            self.error(
                value.span(),
                format!("case value {shown} does not fit the switch type `{ty}`"),
            );
        }
    }

    /// Classifies a `case` label of a union switched on `switch`.
    fn case<'v>(
        &mut self,
        value: &'v Value,
        switch: &TypeSpec,
        switch_named: bool,
    ) -> Option<Case<'v>> {
        let is_bool = *switch == TypeSpec::Bool;
        match value {
            // Lowercase `true` and `false` are accepted, with a warning, as
            // misspellings of `TRUE` and `FALSE`.
            Value::Ident(i) if matches!(i.name.as_str(), "TRUE" | "FALSE" | "true" | "false") => {
                Some(Case::Bool(i.name.eq_ignore_ascii_case("true")))
            }
            _ if is_bool => {
                self.error(
                    value.span(),
                    "a union switched on `bool` takes only `TRUE` and `FALSE`",
                );
                None
            }
            Value::Number(n) => Some(Case::Number(n)),
            // A name that is no constant of this file is taken for a member
            // of the switch type, which may be defined in another file.
            Value::Ident(i) if switch_named && !self.consts.contains_key(&i.name) => {
                Some(Case::Member(i))
            }
            Value::Ident(i) => Some(Case::Const(i)),
        }
    }
}

/// The name of the variant for a `case` label.
fn variant_name(case: &Case<'_>) -> String {
    match case {
        Case::Member(m) => m.name.clone(),
        Case::Bool(true) => "TRUE".into(),
        Case::Bool(false) => "FALSE".into(),
        Case::Number(n) => {
            let text = n.text.trim_start_matches('+').replace('\'', "_");
            match text.strip_prefix('-') {
                Some(abs) => format!("_neg{abs}"),
                None => format!("_{text}"),
            }
        }
        Case::Const(c) => format!("_{}", c.name),
    }
}
