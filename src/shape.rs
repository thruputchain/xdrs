#![allow(unused)]

use core::ops::ControlFlow;

/// A type that wants to visit a host
trait Visitor {
    type Break;
    type VisitStruct: StructVisitor<Self::Break>;

    fn visit_struct(&mut self, name: &str, n_fields: u32) ->
        ControlFlow<Self::Break, Self::VisitStruct>;
}

/// A type that wants to visit a particular struct's fields
trait StructVisitor<B> {
    fn visit_field<T>(&mut self, name: &str, value: T) -> ControlFlow<B>;
}

/// A type that accepts visitors
trait Host {
    /// Accept a `Visitor`, and parade it around the `Host`'s establishment.
    ///
    /// Returns `Err(b)` if the visitor returns `ControlFlow::Break(b)`.
    ///
    fn accept<V>(&self, v: &mut V) -> Result<(), V::Break>
        where
            V: Visitor;
}

#[cfg(test)]
mod test {
    use super::*;

    // Person: Host
    struct Person {
        title: &'static str,
        age: u32,
    }

    impl Host for Person {
        fn accept<V>(&self, v: &mut V) -> Result<(), V::Break>
            where
                V: Visitor
        {
            let mut accept_cf = || -> ControlFlow<<V as Visitor>::Break> {
                let mut s = v.visit_struct("Person", 2)?;

                s.visit_field("title", &self.title)?;
                s.visit_field("age", &self.age)?;

                ControlFlow::Continue(())
            };

            match accept_cf() {
                ControlFlow::Break(b) => Err(b),
                ControlFlow::Continue(()) => Ok(()),
            }
        }
    }

    struct Printer;

    enum PrintError {
        NoInk,
    }

    fn no_ink<T>() -> ControlFlow<PrintError, T> {
        ControlFlow::Break(PrintError::NoInk)
    }

    // Printer: Visitor
    impl Visitor for Printer {
        type Break = PrintError;
        type VisitStruct = PrintStruct;

        fn visit_struct(&mut self, name: &str, n_fields: u32) -> ControlFlow<Self::Break, Self::VisitStruct> {
            no_ink()
        }
    }

    struct PrintStruct;

    impl StructVisitor<PrintError> for PrintStruct {
        fn visit_field<T>(&mut self, name: &str, value: T) -> ControlFlow<PrintError> {
            no_ink()
        }
    }
}
