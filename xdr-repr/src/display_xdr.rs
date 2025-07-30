// Display impls for XDR-spec types

use super::xdr::*;

use std::fmt::{Display, Error, Formatter};

impl Display for Specification {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        for d in &self.definitions {
            d.fmt(f)?;
            f.write_str("\n\n")?;
        }
        Ok(())
    }
}

impl Display for Definition {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        match self {
            Definition::Type(type_def) => type_def.fmt(f),
            Definition::Const(const_def) => const_def.fmt(f),
        }
    }
}

impl Display for TypeDef {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        match self {
            TypeDef::Def(decl) => {
                f.write_str("typedef ")?;
                decl.fmt(f)?;
            }
            TypeDef::Struct(ident, body) => {
                f.write_str("struct ")?;
                ident.fmt(f)?;
                f.write_str(" {\n")?;
                for decl in body.0.iter() {
                    f.write_str("\t")?;
                    decl.fmt(f)?;
                    f.write_str(";")?;
                    f.write_str("\n")?;
                }
                f.write_str("}")?;
            }
        }
        f.write_str(";")?;
        Ok(())
    }
}

impl Display for ConstDef {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        f.write_str("const ")?;
        self.ident.fmt(f)?;
        f.write_str(" = ")?;
        self.constant.fmt(f)?;
        f.write_str(";")?;
        Ok(())
    }
}

impl Display for Ident {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        f.write_str(&self.0)
    }
}

impl Display for Constant {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        self.0.fmt(f)
    }
}

impl Display for Value {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        match self {
            Value::Const(constant) => constant.fmt(f),
            Value::Ident(ident) => ident.fmt(f),
        }
    }
}

impl Display for Declaration {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        match self {
            Declaration::Named { name, ty } => {
                ty.fmt(f)?;
                f.write_str(" ")?;
                name.fmt(f)?;
            }
            Declaration::FixedOpaque { name, size } => {
                f.write_str("opaque ")?;
                name.fmt(f)?;
                f.write_str("[")?;
                size.fmt(f)?;
                f.write_str("]")?;
            }
            Declaration::Void => {
                f.write_str("void")?;
            }
            _ => unimplemented!()
        }
        Ok(())
    }
}

impl Display for TypeSpec {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        match self {
            TypeSpec::Int => {
                f.write_str("int")?;
            }
            TypeSpec::UnsignedInt => {
                f.write_str("unsigned int")?;
            }
            TypeSpec::Ident(ident) => {
                ident.fmt(f)?;
            }
            TypeSpec::Struct(body) => {
                f.write_str("struct ")?;
                body.fmt(f)?;
            }
            _ => panic!("no Display impl for {self:?}")
        }
        Ok(())
    }
}

// pub struct StructBody(pub Vec<Declaration>);
impl Display for StructBody {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        f.write_str("{\n")?;
        for d in &self.0 {
            d.fmt(f)?;
            f.write_str(";\n")?;
        }
        f.write_str("}")
    }
}
