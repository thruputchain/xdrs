// Display impls for XDR-spec types

use super::xdr::*;
use alloc::format;
use alloc::string::String;

const _: () = {
    use core::fmt;

    macro_rules! impl_display {
        { $( $(#[$attr:meta])? $ty:ty ),* $(,)? } => { $(

            $(#[$attr])?
            impl fmt::Display for $ty {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.write_str(&format(self))
                }
            }

        )* };
    }

    impl_display! {
        #[cfg(feature = "ns")]
        Namespace,
        Specification,
        Definition,
        TypeDef,
        ConstDef,
        Ident,
        StructBody,
        EnumBody,
        UnionBody,
        Constant,
        Value,
        Declaration,
        TypeSpec,
    }
};

pub(crate) struct Formatter {
    depth: usize,
    output: String,
}

impl Formatter {
    pub const fn new() -> Self {
        Self {
            depth: 0,
            output: String::new(),
        }
    }

    fn write_str<S: AsRef<str>>(&mut self, s: S) {
        self.output.push_str(s.as_ref())
    }

    #[allow(dead_code)]
    fn writeln(&mut self, s: &str) {
        self.startln();
        self.write_str(s);
        self.endln();
    }

    fn start_indent(&mut self) {
        self.depth += 1;
    }

    fn end_indent(&mut self) {
        self.depth -= 1;
    }

    fn startln(&mut self) {
        for _ in 0..self.depth {
            self.write_str("\t");
        }
    }

    fn endln(&mut self) {
        self.write_str("\n");
    }
}

impl From<Formatter> for String {
    fn from(f: Formatter) -> String {
        f.output
    }
}

pub(crate) trait Format {
    fn fmt(&self, f: &mut Formatter);
}

pub(crate) fn format<T: Format>(x: &T) -> String {
    let mut f = Formatter::new();
    x.fmt(&mut f);
    f.into()
}

#[cfg(feature = "ns")]
impl Format for Namespace {
    fn fmt(&self, f: &mut Formatter) {
        f.write_str("namespace ");
        f.write_str(self.name.as_str());
        f.write_str("\n{\n\n");
        self.spec.fmt(f);
        f.write_str("}\n");
    }
}

impl Format for Specification {
    fn fmt(&self, f: &mut Formatter) {
        for d in &self.definitions {
            d.fmt(f);
            f.write_str("\n");
        }
    }
}

impl Format for Definition {
    fn fmt(&self, f: &mut Formatter) {
        match self {
            Definition::Type(type_def) => type_def.fmt(f),
            Definition::Const(const_def) => const_def.fmt(f),
        }
    }
}

impl Format for TypeDef {
    fn fmt(&self, f: &mut Formatter) {
        match self {
            TypeDef::Def(decl) => {
                f.write_str("typedef ");
                decl.fmt(f);
            }
            TypeDef::Struct(ident, body) => {
                f.write_str("struct ");
                ident.fmt(f);
                f.write_str(" ");
                body.fmt(f);
            }
            TypeDef::Enum(ident, body) => {
                f.write_str("enum ");
                ident.fmt(f);
                f.write_str(" ");
                body.fmt(f);
            }
            TypeDef::Union(ident, body) => {
                f.write_str("union ");
                ident.fmt(f);
                f.write_str(" ");
                body.fmt(f);
            }
        }
        f.write_str(";\n");
    }
}

impl Format for ConstDef {
    fn fmt(&self, f: &mut Formatter) {
        f.write_str("const ");
        self.ident.fmt(f);
        f.write_str(" = ");
        self.constant.fmt(f);
        f.write_str(";\n");
    }
}

impl Format for Ident {
    fn fmt(&self, f: &mut Formatter) {
        f.write_str(&self.0);
    }
}

impl Format for Constant {
    fn fmt(&self, f: &mut Formatter) {
        match self {
            Self::Signed(n) => f.write_str(format!("{n}")),
            Self::Unsigned(n) => f.write_str(format!("{n}")),
        }
    }
}

impl Format for Value {
    fn fmt(&self, f: &mut Formatter) {
        match self {
            Value::Const(constant) => constant.fmt(f),
            Value::Ident(ident) => ident.fmt(f),
        }
    }
}

impl Format for Declaration {
    fn fmt(&self, f: &mut Formatter) {
        match self {
            Declaration::Named { name, ty } => {
                ty.fmt(f);
                f.write_str(" ");
                name.fmt(f);
            }
            Declaration::FixedOpaque { name, size } => {
                assert_non_negative_bound(size);
                f.write_str("opaque ");
                name.fmt(f);
                f.write_str("[");
                size.fmt(f);
                f.write_str("]");
            }
            Declaration::VariableOpaque { name, max } => {
                assert_non_negative_bound(max);
                f.write_str("opaque ");
                name.fmt(f);
                f.write_str("<");
                if max != &Value::Const(Constant::Unsigned(u32::MAX)) {
                    max.fmt(f);
                }
                f.write_str(">");
            }
            Declaration::String { name, max } => {
                assert_non_negative_bound(max);
                f.write_str("string ");
                name.fmt(f);
                f.write_str("<");
                if max != &Value::Const(Constant::Unsigned(u32::MAX)) {
                    max.fmt(f);
                }
                f.write_str(">");
            }
            Declaration::FixedArray { name, elt_ty, len } => {
                assert_non_negative_bound(len);
                elt_ty.fmt(f);
                f.write_str(" ");
                name.fmt(f);
                f.write_str("[");
                len.fmt(f);
                f.write_str("]");
            }
            Declaration::VariableArray { name, elt_ty, max } => {
                assert_non_negative_bound(max);
                elt_ty.fmt(f);
                f.write_str(" ");
                name.fmt(f);
                f.write_str("<");
                if max != &Value::Const(Constant::Unsigned(u32::MAX)) {
                    max.fmt(f);
                }
                f.write_str(">");
            }
            Declaration::Optional { name, opt_ty } => {
                opt_ty.fmt(f);
                f.write_str(" *");
                name.fmt(f);
            }
            Declaration::Void => {
                f.write_str("void");
            }
        }
    }
}

impl Format for TypeSpec {
    fn fmt(&self, f: &mut Formatter) {
        match self {
            TypeSpec::Int => {
                f.write_str("int");
            }
            TypeSpec::Unsigned => {
                f.write_str("unsigned int");
            }
            TypeSpec::Hyper => {
                f.write_str("hyper");
            }
            TypeSpec::UnsignedHyper => {
                f.write_str("unsigned hyper");
            }
            TypeSpec::Float => {
                f.write_str("float");
            }
            TypeSpec::Double => {
                f.write_str("double");
            }
            TypeSpec::Quadruple => {
                f.write_str("quadruple");
            }
            TypeSpec::Bool => {
                f.write_str("bool");
            }
            TypeSpec::Ident(ident) => {
                ident.fmt(f);
            }
            TypeSpec::Struct(body) => {
                f.write_str("struct ");
                body.fmt(f);
            }
            TypeSpec::Enum(body) => {
                f.write_str("enum ");
                body.fmt(f);
            }
            TypeSpec::Union(body) => {
                f.write_str("union ");
                body.fmt(f);
            }
        }
    }
}

impl Format for StructBody {
    fn fmt(&self, f: &mut Formatter) {
        f.write_str("{");
        f.endln();
        f.start_indent();
        for d in self.fields() {
            f.startln();
            d.fmt(f);
            f.write_str(";");
            f.endln();
        }
        f.end_indent();
        f.startln();
        f.write_str("}");
    }
}

impl Format for UnionBody {
    fn fmt(&self, f: &mut Formatter) {
        f.write_str("switch (");
        self.discriminant_decl.fmt(f);
        f.write_str(")");
        f.endln();

        f.startln();
        f.write_str("{");
        f.endln();
        for case_spec in &self.case_specs {
            for value in &case_spec.cases {
                f.startln();
                f.write_str("case ");
                value.fmt(f);
                f.write_str(":");
                f.endln();
            }

            f.start_indent();
            {
                f.startln();
                case_spec.decl.fmt(f);
                f.write_str(";");
                f.endln();
            }
            f.end_indent();
        }
        if let Some(decl) = &self.default_decl {
            f.startln();
            f.write_str("default:");
            f.endln();

            f.start_indent();
            {
                f.startln();
                decl.fmt(f);
                f.write_str(";");
                f.endln();
            }
            f.end_indent();
        }
        f.startln();
        f.write_str("}");
    }
}

impl Format for EnumBody {
    fn fmt(&self, f: &mut Formatter) {
        f.write_str("{");
        f.endln();
        f.start_indent();
        for (ident, value) in &self.0 {
            f.startln();
            ident.fmt(f);
            f.write_str(" = ");
            value.fmt(f);
            f.write_str(",");
            f.endln();
        }
        f.end_indent();
        f.startln();
        f.write_str("}");
    }
}
