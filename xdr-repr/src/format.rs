// Display impls for XDR-spec types

use super::xdr::*;

const _: () = {
    use std::fmt;

    macro_rules! impl_display {
        { $($ty:ty),* $(,)? } => { $(

            impl fmt::Display for $ty {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.write_str(&format(self))
                }
            }

        )* };
    }

    impl_display! {
        Specification,
        Definition,
        TypeDef,
        ConstDef,
        Ident,
        StructBody,
        EnumBody,
        // UnionBody,
        Constant,
        Value,
        Declaration,
        TypeSpec,
    }
};

pub struct Formatter {
    depth: usize,
    output: String,
}

impl Formatter {
    pub const fn new() -> Self {
        Self {
            depth: 0,
            output: String::new()
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

pub trait Format {
    fn fmt(&self, f: &mut Formatter);
}

pub fn format<T: Format>(x: &T) -> String {
    let mut f = Formatter::new();
    x.fmt(&mut f);
    f.into()
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
                f.write_str("opaque ");
                name.fmt(f);
                f.write_str("[");
                size.fmt(f);
                f.write_str("]");
            }
            Declaration::VariableOpaque { name, max } => {
                f.write_str("opaque ");
                name.fmt(f);
                f.write_str("<");
                max.fmt(f);
                f.write_str(">");
            }
            Declaration::Void => {
                f.write_str("void");
            }
            _ => unimplemented!()
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
            _ => panic!("Format unimplemented for {self:?}")
        }
    }
}

impl Format for StructBody {
    fn fmt(&self, f: &mut Formatter) {
        f.write_str("{");
        f.endln();
        f.start_indent();
        for d in &self.0 {
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
            f.write_str(";");
            f.endln();
        }
        f.end_indent();
        f.startln();
        f.write_str("}");
    }
}
