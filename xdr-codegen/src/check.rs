//! Checks on the names and values in a parsed file, which the grammar does
//! not express:
//!
//! - Every type name is distinct across the file, namespaces included.
//! - Warnings for `int32_t`, `uint32_t`, `int64_t` and `uint64_t` as type
//!   names, and for lowercase `true` and `false` as case labels.

use std::collections::HashSet;

use crate::ast::*;
use crate::diag::Diagnostic;

pub(crate) fn check(items: &[Item], diags: &mut Vec<Diagnostic>) {
    Checker {
        type_names: HashSet::new(),
        diags,
    }
    .items(items);
}

struct Checker<'a, 'd> {
    /// The type names declared so far.
    type_names: HashSet<&'a str>,
    diags: &'d mut Vec<Diagnostic>,
}

impl<'a> Checker<'a, '_> {
    fn items(&mut self, items: &'a [Item]) {
        for item in items {
            match &item.kind {
                ItemKind::Typedef(decl) => {
                    self.decl(decl);
                    self.declare_type(item, "typedef", &decl.name);
                }
                ItemKind::Enum(e) => self.declare_type(item, "enum", &e.name),
                ItemKind::Struct(s) => {
                    self.struct_body(&s.body);
                    self.declare_type(item, "struct", &s.name);
                }
                ItemKind::Union(u) => {
                    self.union_body(&u.body);
                    self.declare_type(item, "union", &u.name);
                }
                ItemKind::Namespace(ns) => self.items(&ns.body),
                ItemKind::Program(program) => {
                    for proc in program.versions.iter().flat_map(|v| &v.procs) {
                        proc.result
                            .iter()
                            .chain(&proc.args)
                            .for_each(|t| self.ty(t));
                    }
                }
                ItemKind::Const(_) | ItemKind::Literal(_) => {}
            }
        }
    }

    /// Records a type's name, reporting it if it is already taken.
    fn declare_type(&mut self, item: &Item, what: &str, name: &'a Ident) {
        if !self.type_names.insert(&name.name) {
            self.diags.push(Diagnostic::error(
                item.span,
                format!("duplicate declaration of {what} `{}`", name.name),
            ));
        }
    }

    fn struct_body(&mut self, body: &StructBody) {
        for field in &body.fields {
            self.member(field);
        }
    }

    fn union_body(&mut self, body: &UnionBody) {
        self.ty(&body.switch_type);
        for arm in &body.arms {
            for case in &arm.cases {
                if let Value::Ident(i) = case
                    && (i.name == "true" || i.name == "false")
                {
                    self.diags.push(Diagnostic::warning(
                        i.span,
                        "RFC 4506 spells the boolean constants TRUE and FALSE",
                    ));
                }
            }
            self.member(&arm.member);
        }
    }

    fn member(&mut self, member: &Member) {
        if let Some(decl) = &member.decl {
            self.decl(decl);
        }
    }

    fn decl(&mut self, decl: &Declaration) {
        match &decl.ty {
            DeclType::Plain(t)
            | DeclType::Optional(t)
            | DeclType::Array { elem: t, .. }
            | DeclType::VarArray { elem: t, .. } => self.ty(t),
            DeclType::Opaque { .. } | DeclType::VarOpaque { .. } | DeclType::String { .. } => {}
        }
    }

    fn ty(&mut self, ty: &TypeSpec) {
        match ty {
            TypeSpec::Named(ident) => {
                let instead = match ident.name.as_str() {
                    "int32_t" => "int",
                    "uint32_t" => "unsigned int",
                    "int64_t" => "hyper",
                    "uint64_t" => "unsigned hyper",
                    _ => return,
                };
                self.diags.push(Diagnostic::warning(
                    ident.span,
                    format!("use `{instead}` in XDR source to get `{}`", ident.name),
                ));
            }
            TypeSpec::Struct(body) => self.struct_body(body),
            TypeSpec::Union(body) => self.union_body(body),
            _ => {}
        }
    }
}
