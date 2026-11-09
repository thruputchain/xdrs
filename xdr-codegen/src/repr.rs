//! Lowering a parsed file to an [`xdr_repr::Specification`], and reducing a
//! specification to the wire format it describes.
//!
//! The Rust that xdrc generates can describe itself through
//! `#[derive(XDR)]`, under the `spec` feature: each generated module has an
//! `xdr_spec()` collecting what the derive gives its types. That description
//! is not the source. It has lost typedef names, named bounds, enum members
//! (an enum becomes a newtype) and comments, and it describes each union's
//! switch by an enum it synthesizes. But it should describe the same wire
//! format. [`lower`] gives the source as a [`Specification`], and
//! [`wire_shapes`] reduces either side to a [`Shape`] per named type, which
//! can be compared.

use std::collections::{BTreeMap, HashMap};

use xdr_repr::{
    CaseSpec, ConstDef, Constant, Declaration as XDecl, Definition, EnumBody as XEnumBody,
    Ident as XIdent, Specification, StructBody as XStructBody, TypeDef, TypeSpec as XTypeSpec,
    UnionBody as XUnionBody, Value as XValue,
};

use crate::ast::*;
use crate::diag::Diagnostic;

/// Lowers a parsed file to an [`xdr_repr::Specification`].
///
/// Namespaces are flattened, as XDR names are global anyway. `%` lines and
/// comments are dropped, and so are programs, which a `Specification` does
/// not hold. An unbounded `<>` becomes a maximum of `u32::MAX`, as
/// `xdr-repr` represents it. A number that does not fit 32 bits is an
/// error, as `xdr-repr` cannot hold it.
pub fn lower(spec: &Spec) -> Result<Specification, Vec<Diagnostic>> {
    let mut lowerer = Lowerer { diags: Vec::new() };
    let mut definitions = Vec::new();
    lowerer.items(&spec.items, &mut definitions);
    if lowerer.diags.is_empty() {
        Ok(Specification { definitions })
    } else {
        Err(lowerer.diags)
    }
}

struct Lowerer {
    diags: Vec<Diagnostic>,
}

fn ident(i: &Ident) -> XIdent {
    XIdent(i.name.clone())
}

impl Lowerer {
    fn items(&mut self, items: &[Item], out: &mut Vec<Definition>) {
        for item in items {
            let def = match &item.kind {
                ItemKind::Typedef(decl) => TypeDef::Def(self.decl(decl)),
                ItemKind::Enum(e) => TypeDef::Enum(ident(&e.name), self.enum_body(&e.body)),
                ItemKind::Struct(s) => TypeDef::Struct(ident(&s.name), self.struct_body(&s.body)),
                ItemKind::Union(u) => TypeDef::Union(ident(&u.name), self.union_body(&u.body)),
                ItemKind::Const(c) => {
                    if let Some(constant) = self.constant(&c.value) {
                        out.push(Definition::Const(ConstDef {
                            ident: ident(&c.name),
                            constant,
                        }));
                    }
                    continue;
                }
                ItemKind::Namespace(ns) => {
                    self.items(&ns.body, out);
                    continue;
                }
                ItemKind::Program(_) | ItemKind::Literal(_) => continue,
            };
            out.push(Definition::Type(def));
        }
    }

    fn constant(&mut self, n: &Number) -> Option<Constant> {
        if let Ok(v) = u32::try_from(n.value) {
            Some(Constant::Unsigned(v))
        } else if let Ok(v) = i32::try_from(n.value) {
            Some(Constant::Signed(v))
        } else {
            self.diags.push(Diagnostic::error(
                n.span,
                format!("`{}` does not fit 32 bits, which xdr-repr requires", n.text),
            ));
            None
        }
    }

    fn value(&mut self, v: &Value) -> XValue {
        match v {
            Value::Ident(i) => XValue::Ident(ident(i)),
            Value::Number(n) => XValue::Const(self.constant(n).unwrap_or(Constant::Unsigned(0))),
        }
    }

    fn bound(&mut self, max: &Option<Value>) -> XValue {
        match max {
            Some(v) => self.value(v),
            None => XValue::Const(Constant::Unsigned(u32::MAX)),
        }
    }

    fn decl(&mut self, d: &Declaration) -> XDecl {
        let name = ident(&d.name);
        match &d.ty {
            DeclType::Plain(t) => XDecl::Named {
                name,
                ty: self.type_spec(t),
            },
            DeclType::Optional(t) => XDecl::Optional {
                name,
                opt_ty: self.type_spec(t),
            },
            DeclType::Array { elem, len } => XDecl::FixedArray {
                name,
                elt_ty: self.type_spec(elem),
                len: self.value(len),
            },
            DeclType::VarArray { elem, max } => XDecl::VariableArray {
                name,
                elt_ty: self.type_spec(elem),
                max: self.bound(max),
            },
            DeclType::Opaque { len } => XDecl::FixedOpaque {
                name,
                size: self.value(len),
            },
            DeclType::VarOpaque { max } => XDecl::VariableOpaque {
                name,
                max: self.bound(max),
            },
            DeclType::String { max } => XDecl::String {
                name,
                max: self.bound(max),
            },
        }
    }

    fn member(&mut self, m: &Member) -> XDecl {
        match &m.decl {
            Some(d) => self.decl(d),
            None => XDecl::Void,
        }
    }

    fn type_spec(&mut self, t: &TypeSpec) -> XTypeSpec {
        match t {
            TypeSpec::Int => XTypeSpec::Int,
            TypeSpec::UnsignedInt => XTypeSpec::Unsigned,
            TypeSpec::Hyper => XTypeSpec::Hyper,
            TypeSpec::UnsignedHyper => XTypeSpec::UnsignedHyper,
            TypeSpec::Float => XTypeSpec::Float,
            TypeSpec::Double => XTypeSpec::Double,
            TypeSpec::Quadruple => XTypeSpec::Quadruple,
            TypeSpec::Bool => XTypeSpec::Bool,
            TypeSpec::Named(i) => XTypeSpec::Ident(ident(i)),
            TypeSpec::Enum(b) => XTypeSpec::Enum(self.enum_body(b)),
            TypeSpec::Struct(b) => XTypeSpec::Struct(self.struct_body(b)),
            TypeSpec::Union(b) => XTypeSpec::Union(Box::new(self.union_body(b))),
        }
    }

    fn enum_body(&mut self, b: &EnumBody) -> XEnumBody {
        XEnumBody(
            b.members
                .iter()
                .map(|m| (ident(&m.name), self.value(&m.value)))
                .collect(),
        )
    }

    fn struct_body(&mut self, b: &StructBody) -> XStructBody {
        XStructBody::new(b.fields.iter().map(|f| self.member(f)).collect())
    }

    /// Labels on the default arm, which x23c allows, are dropped: the
    /// default arm takes those values anyway.
    fn union_body(&mut self, b: &UnionBody) -> XUnionBody {
        let mut case_specs = Vec::new();
        let mut default_decl = None;
        for arm in &b.arms {
            let decl = self.member(&arm.member);
            if arm.is_default {
                default_decl = Some(decl);
            } else {
                let cases = arm.cases.iter().map(|c| self.value(c)).collect();
                case_specs.push(CaseSpec { cases, decl });
            }
        }
        XUnionBody {
            discriminant_decl: XDecl::Named {
                name: ident(&b.switch_name),
                ty: self.type_spec(&b.switch_type),
            },
            case_specs,
            default_decl,
        }
    }
}

/// The wire format of a type, with names, typedefs and constants resolved.
///
/// Types that encode alike reduce to the same shape: `int`, `unsigned
/// int`, `bool` and every enum are a [`Shape::Word`]; a struct is the
/// sequence of its fields, flattened into an enclosing struct; and a union
/// whose arms are all `void`, with no default, is just its discriminant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shape {
    /// Four bytes: `int`, `unsigned int`, `bool` or an enum.
    Word,
    /// Eight bytes: `hyper` or `unsigned hyper`.
    Hyper,
    /// `float`
    Float,
    /// `double`
    Double,
    /// `quadruple`
    Quadruple,
    /// `opaque[N]`
    Opaque(u32),
    /// `opaque<MAX>`
    VarOpaque(u32),
    /// `string<MAX>`
    String(u32),
    /// `T[N]`
    Array(Box<Shape>, u32),
    /// `T<MAX>`
    VarArray(Box<Shape>, u32),
    /// `T *`
    Optional(Box<Shape>),
    /// A struct's fields in order, or `void` if there are none. Never holds
    /// one field, or another sequence.
    Seq(Vec<Shape>),
    /// A discriminant, then the arm for its value, keyed by its 32 bits.
    Union {
        /// The arm for each case value.
        arms: BTreeMap<u32, Shape>,
        /// The default arm, if any.
        default: Option<Box<Shape>>,
    },
    /// A reference back to the named type being reduced, which contains
    /// itself.
    Recursive(String),
    /// A named type that the specification does not define, such as one
    /// written in Rust, kept by name.
    Undefined(String),
}

impl Shape {
    fn void() -> Shape {
        Shape::Seq(Vec::new())
    }

    /// The sequence of `shapes`, flattened.
    fn seq(shapes: impl IntoIterator<Item = Shape>) -> Shape {
        let mut flat = Vec::new();
        for shape in shapes {
            match shape {
                Shape::Seq(inner) => flat.extend(inner),
                shape => flat.push(shape),
            }
        }
        if flat.len() == 1 {
            flat.pop().expect("one shape")
        } else {
            Shape::Seq(flat)
        }
    }
}

/// The wire shapes of the named types in a specification.
#[derive(Clone, Debug, Default)]
pub struct WireShapes {
    /// The shape of each type that could be reduced, by name.
    pub shapes: BTreeMap<String, Shape>,
    /// Why each other type could not be, by name: for example, a constant
    /// it uses is not defined.
    pub errors: BTreeMap<String, String>,
}

/// The wire shape of each named type that `spec` defines.
///
/// Constants and enum members are resolved by name. An undefined type is
/// kept by name, but a type that uses an undefined constant cannot be
/// reduced. A union's case labels are looked up first among the members of
/// its switch type, as a derived specification reuses member names across
/// the enums it synthesizes. Fails only if `spec` defines a name twice,
/// differently.
pub fn wire_shapes(spec: &Specification) -> Result<WireShapes, String> {
    let mut reducer = Reducer {
        types: HashMap::new(),
        consts: HashMap::new(),
        members: HashMap::new(),
        stack: Vec::new(),
    };
    for definition in &spec.definitions {
        match definition {
            Definition::Const(c) => {
                reducer
                    .consts
                    .insert(c.ident.0.clone(), constant(&c.constant));
            }
            Definition::Type(def) => {
                let name = match def {
                    TypeDef::Def(decl) => match decl.name() {
                        Some(name) => name.0.clone(),
                        None => continue,
                    },
                    TypeDef::Enum(name, _) | TypeDef::Struct(name, _) | TypeDef::Union(name, _) => {
                        name.0.clone()
                    }
                };
                if let Some(old) = reducer.types.insert(name.clone(), def)
                    && old != def
                {
                    return Err(format!("`{name}` is defined twice, differently"));
                }
            }
        }
    }
    reducer.collect_members(spec);
    let mut names: Vec<String> = reducer.types.keys().cloned().collect();
    names.sort();
    let mut out = WireShapes::default();
    for name in names {
        match reducer.named(&name) {
            Ok(shape) => {
                out.shapes.insert(name, shape);
            }
            Err(e) => {
                out.errors.insert(name, e);
            }
        }
    }
    Ok(out)
}

fn constant(c: &Constant) -> i64 {
    match c {
        Constant::Unsigned(v) => i64::from(*v),
        Constant::Signed(v) => i64::from(*v),
    }
}

struct Reducer<'s> {
    types: HashMap<String, &'s TypeDef>,
    consts: HashMap<String, i64>,
    /// Every enum member, or `None` for a name that several enums use with
    /// different values.
    members: HashMap<String, Option<i64>>,
    /// The named types being reduced, to cut recursion.
    stack: Vec<String>,
}

impl<'s> Reducer<'s> {
    fn collect_members(&mut self, spec: &'s Specification) {
        fn walk_type(ty: &XTypeSpec, out: &mut Vec<(String, i64)>, consts: &HashMap<String, i64>) {
            match ty {
                XTypeSpec::Enum(body) => {
                    for (name, value) in &body.0 {
                        let value = match value {
                            XValue::Const(c) => constant(c),
                            XValue::Ident(i) => match consts.get(&i.0) {
                                Some(v) => *v,
                                None => continue,
                            },
                        };
                        out.push((name.0.clone(), value));
                    }
                }
                XTypeSpec::Struct(body) => {
                    body.fields().iter().for_each(|d| walk_decl(d, out, consts))
                }
                XTypeSpec::Union(body) => walk_union(body, out, consts),
                _ => {}
            }
        }
        fn walk_decl(d: &XDecl, out: &mut Vec<(String, i64)>, consts: &HashMap<String, i64>) {
            match d {
                XDecl::Named { ty, .. }
                | XDecl::FixedArray { elt_ty: ty, .. }
                | XDecl::VariableArray { elt_ty: ty, .. }
                | XDecl::Optional { opt_ty: ty, .. } => walk_type(ty, out, consts),
                _ => {}
            }
        }
        fn walk_union(u: &XUnionBody, out: &mut Vec<(String, i64)>, consts: &HashMap<String, i64>) {
            walk_decl(&u.discriminant_decl, out, consts);
            u.case_specs
                .iter()
                .for_each(|c| walk_decl(&c.decl, out, consts));
            u.default_decl
                .iter()
                .for_each(|d| walk_decl(d, out, consts));
        }

        let mut found = Vec::new();
        for definition in &spec.definitions {
            if let Definition::Type(def) = definition {
                match def {
                    TypeDef::Def(d) => walk_decl(d, &mut found, &self.consts),
                    TypeDef::Enum(_, body) => {
                        walk_type(&XTypeSpec::Enum(body.clone()), &mut found, &self.consts)
                    }
                    TypeDef::Struct(_, body) => {
                        walk_type(&XTypeSpec::Struct(body.clone()), &mut found, &self.consts)
                    }
                    TypeDef::Union(_, body) => walk_union(body, &mut found, &self.consts),
                }
            }
        }
        for (name, value) in found {
            self.members
                .entry(name)
                .and_modify(|old| {
                    if *old != Some(value) {
                        *old = None;
                    }
                })
                .or_insert(Some(value));
        }
    }

    /// The shape of the named type `name`.
    fn named(&mut self, name: &str) -> Result<Shape, String> {
        if self.stack.iter().any(|n| n == name) {
            return Ok(Shape::Recursive(name.to_owned()));
        }
        let Some(&def) = self.types.get(name) else {
            return Ok(Shape::Undefined(name.to_owned()));
        };
        self.stack.push(name.to_owned());
        let shape = match def {
            TypeDef::Def(decl) => self.decl(decl),
            TypeDef::Enum(..) => Ok(Shape::Word),
            TypeDef::Struct(_, body) => self.struct_body(body),
            TypeDef::Union(_, body) => self.union_body(body),
        };
        self.stack.pop();
        shape.map_err(|e| format!("{e}, in `{name}`"))
    }

    fn eval(&self, v: &XValue, members: Option<&XEnumBody>) -> Result<i64, String> {
        match v {
            XValue::Const(c) => Ok(constant(c)),
            XValue::Ident(i) => {
                let name = i.0.as_str();
                if let Some((_, value)) =
                    members.and_then(|m| m.0.iter().find(|(n, _)| n.0 == name))
                {
                    return self.eval(value, None);
                }
                if let Some(v) = self.consts.get(name) {
                    return Ok(*v);
                }
                match (self.members.get(name), name) {
                    (Some(Some(v)), _) => Ok(*v),
                    (Some(None), _) => Err(format!("`{name}` is ambiguous")),
                    // Lowercase too, which xdrc accepts with a warning.
                    (None, "TRUE" | "true") => Ok(1),
                    (None, "FALSE" | "false") => Ok(0),
                    (None, _) => Err(format!("`{name}` is not defined")),
                }
            }
        }
    }

    fn size(&self, v: &XValue) -> Result<u32, String> {
        let n = self.eval(v, None)?;
        u32::try_from(n).map_err(|_| format!("`{n}` is not a valid size"))
    }

    fn decl(&mut self, d: &XDecl) -> Result<Shape, String> {
        Ok(match d {
            XDecl::Named { ty, .. } => self.type_spec(ty)?,
            XDecl::FixedOpaque { size, .. } => Shape::Opaque(self.size(size)?),
            XDecl::VariableOpaque { max, .. } => Shape::VarOpaque(self.size(max)?),
            XDecl::String { max, .. } => Shape::String(self.size(max)?),
            XDecl::FixedArray { elt_ty, len, .. } => {
                Shape::Array(Box::new(self.type_spec(elt_ty)?), self.size(len)?)
            }
            XDecl::VariableArray { elt_ty, max, .. } => {
                Shape::VarArray(Box::new(self.type_spec(elt_ty)?), self.size(max)?)
            }
            XDecl::Optional { opt_ty, .. } => Shape::Optional(Box::new(self.type_spec(opt_ty)?)),
            XDecl::Void => Shape::void(),
        })
    }

    fn type_spec(&mut self, t: &XTypeSpec) -> Result<Shape, String> {
        Ok(match t {
            XTypeSpec::Int | XTypeSpec::Unsigned | XTypeSpec::Bool | XTypeSpec::Enum(_) => {
                Shape::Word
            }
            XTypeSpec::Hyper | XTypeSpec::UnsignedHyper => Shape::Hyper,
            XTypeSpec::Float => Shape::Float,
            XTypeSpec::Double => Shape::Double,
            XTypeSpec::Quadruple => Shape::Quadruple,
            XTypeSpec::Struct(body) => self.struct_body(body)?,
            XTypeSpec::Union(body) => self.union_body(body)?,
            XTypeSpec::Ident(i) => self.named(&i.0)?,
        })
    }

    fn struct_body(&mut self, body: &XStructBody) -> Result<Shape, String> {
        let fields = body
            .fields()
            .iter()
            .map(|d| self.decl(d))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Shape::seq(fields))
    }

    /// The members of the enum that `ty` names, following typedefs.
    fn switch_members(&self, ty: &XTypeSpec) -> Option<XEnumBody> {
        let mut ty = ty.clone();
        for _ in 0..=self.types.len() {
            match ty {
                XTypeSpec::Enum(body) => return Some(body),
                XTypeSpec::Ident(i) => match self.types.get(&i.0)? {
                    TypeDef::Enum(_, body) => return Some(body.clone()),
                    TypeDef::Def(XDecl::Named { ty: next, .. }) => ty = next.clone(),
                    _ => return None,
                },
                _ => return None,
            }
        }
        None
    }

    fn union_body(&mut self, body: &XUnionBody) -> Result<Shape, String> {
        let switch = match &body.discriminant_decl {
            XDecl::Named { ty, .. } => ty,
            _ => return Err("a union's discriminant is not a named declaration".into()),
        };
        // A switch type defined elsewhere is taken to be 32 bits.
        if !matches!(self.type_spec(switch)?, Shape::Word | Shape::Undefined(_)) {
            return Err("a union's discriminant is not 32 bits".into());
        }
        let members = self.switch_members(switch);
        let mut arms = BTreeMap::new();
        for case in &body.case_specs {
            let shape = self.decl(&case.decl)?;
            for value in &case.cases {
                // Keep the 32 bits, whether the value is signed or not.
                let key = self.eval(value, members.as_ref())? as u32;
                if arms.insert(key, shape.clone()).is_some() {
                    return Err(format!("case value {key} appears twice"));
                }
            }
        }
        let default = match &body.default_decl {
            Some(d) => Some(Box::new(self.decl(d)?)),
            None => None,
        };
        if default.is_none() && arms.values().all(|s| *s == Shape::void()) {
            // Encodes as its discriminant alone, like an enum.
            return Ok(Shape::Word);
        }
        Ok(Shape::Union { arms, default })
    }
}
