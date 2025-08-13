// Display impls for RPC-spec types

use super::rpc::*;
use core::fmt::{Display, Formatter, Result};

impl Display for Id {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        self.0.fmt(f)
    }
}

impl Display for Constant {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        self.0.fmt(f)
    }
}

impl Display for Procedure {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        f.write_str("\t\t")?;
        if let Some(ty) = &self.return_type {
            ty.fmt(f)?;
        } else {
            "void".fmt(f)?;
        }
        writeln!(f, " {} (", self.id)?;

        let mut arg_types = self.arg_types.iter().peekable();
        while let Some(arg_type) = arg_types.next() {
            let ty = &arg_type.ty;
            let name = arg_type.name.as_str();
            write!(f, "\t\t\t{ty} /* {name} */")?;
            if arg_types.peek().is_some() {
                f.write_str(",")?;
            }
            f.write_str("\n")?;
        }

        writeln!(f, "\t\t) = {};", self.value)
    }
}

impl Display for Version {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        f.write_str("\t")?;
        writeln!(f, "version {} {{", self.id)?;

        for p in self.procedures.iter() {
            write!(f, "{p}")?;
        }

        writeln!(f, "\t}} = {};", self.value)
    }
}

impl Display for Program {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        writeln!(f, "program {} {{", self.id)?;

        for v in self.versions.iter() {
            v.fmt(f)?;
        }

        writeln!(f, "}} = {};", self.value)
    }
}
