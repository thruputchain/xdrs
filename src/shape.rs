//!
//! Traits [`Host`] and [`Visitor`] for guiding a visitor
//! through the structure of an XDR value.
//!

#![allow(unused)]

/// A type that wants to visit a host
pub trait Visitor {
    /// A type the `Visitor` returns when it wants to end the visit
    type Error;

    /// A type that will visit the parts of a struct
    type VisitStruct<'s>: StructVisitor<Self::Error>
    where
        Self: 's;

    /// The final result of a visit
    type Return;

    /// Visit an XDR structure
    fn visit_struct<'a>(
        &'a mut self,
        name: &str,
        n_fields: u32,
    ) -> Result<Self::VisitStruct<'a>, Self::Error>;

    /// Yield the final result, consuming the `Visitor`
    fn into_return(self) -> Self::Return;
}

/// A type that wants to visit an XDR structure's fields
pub trait StructVisitor<E> {
    /// Visit a field of an XDR structure
    fn visit_field<T>(&mut self, name: &str, value: T) -> Result<(), E>;
}

/// A type that accepts visitors
pub trait Host {
    /// Accept a [`Visitor`], and guide it around the [`Host`]'s establishment.
    ///
    /// Return the visitor's final result if the tour went well.
    ///
    /// Return `Err(b)` if the visitor pleads for a `ControlFlow::Break(b)`.
    ///
    fn accept<V>(&self, v: V) -> Result<V::Return, V::Error>
    where
        V: Visitor;
}

#[cfg(test)]
mod test {
    extern crate std;

    use super::*;

    // Create a `Person` host-object and ask it to accept a `Printer` visitor, capturing the
    // result.
    #[test]
    pub fn perform_tour() {
        let host = host::Person::new("Dave", 42);

        let visitor = visitor::Printer::new();

        let result = host
            .accept(visitor)
            .unwrap_or_else(|b| panic!("Visit failed: {}", b));

        assert_eq!(result, "Person { title: V0, age: V1 }");
    }

    mod host {
        use super::*;

        // The type whose XDR representation will be visited
        pub struct Person {
            title: &'static str,
            age: u32,
        }

        impl Person {
            pub fn new(title: &'static str, age: u32) -> Self {
                Self { title, age }
            }

            // Guide the visitor to each XDR component of a `Person` value
            fn guide<V>(&self, v: &mut V) -> Result<(), V::Error>
            where
                V: Visitor
            {
                let mut s = v.visit_struct("Person", 2)?;

                s.visit_field("title", &self.title)?;
                s.visit_field("age", &self.age)?;

                Ok(())
            }
        }

        impl Host for Person {
            // Accept a visitor and guide it to each XDR component of a `Person` value,
            // returning the recorded result.
            fn accept<V>(&self, mut v: V) -> Result<V::Return, V::Error>
            where
                V: Visitor,
            {
                self.guide(&mut v)?;

                Ok(v.into_return())
            }
        }
    }

    mod visitor {
        use super::*;
        use core::fmt::{self, Write};
        use core::fmt::Error as WriteError;
        use heapless::String;
        use thiserror::Error;

        #[derive(Debug, Error)]
        pub enum PrintError {
            #[error("Write error: {0}")]
            Write(#[from] fmt::Error)
        }

        type PrintResult<T = ()> = Result<T, PrintError>;

        type Story = String<1000>;

        pub struct Printer {
            story: Story,
        }

        impl Printer {
            pub fn new() -> Self {
                Self {
                    story: String::new(),
                }
            }

            pub fn writer(&mut self) -> &mut impl Write {
                &mut self.story
            }
        }

        impl Visitor for Printer {
            type Error = PrintError;
            type VisitStruct<'a> = PrintStruct<'a>;
            type Return = Story;

            fn visit_struct<'a>(
                &'a mut self,
                name: &str,
                n_fields: u32,
            ) -> PrintResult<PrintStruct<'a>> {
                write!(self.writer(), "{name} {{ ")?;

                Ok(PrintStruct::new(n_fields, self))
            }

            fn into_return(self) -> Story {
                self.story
            }
        }

        pub struct PrintStruct<'a> {
            n_fields: u32,
            n_fields_visited: u32,
            printer: &'a mut Printer,
        }

        impl<'a> PrintStruct<'a> {
            pub fn new(n_fields: u32, printer: &'a mut Printer) -> Self {
                Self {
                    n_fields,
                    n_fields_visited: 0,
                    printer,
                }
            }

            pub fn writer(&mut self) -> &mut impl Write {
                self.printer.writer()
            }
        }

        impl StructVisitor<PrintError> for PrintStruct<'_> {
            fn visit_field<T>(&mut self, name: &str, value: T) -> PrintResult {
                let index = self.n_fields_visited;
                let n_fields = self.n_fields;

                assert!(index < n_fields, "Too many fields");

                write!(
                    self.writer(),
                    "{}: V{}{} ",
                    name,
                    index,
                    if index < n_fields - 1 { "," } else { "" }
                );

                self.n_fields_visited += 1;

                Ok(())
            }
        }

        impl core::ops::Drop for PrintStruct<'_> {
            fn drop(&mut self) {
                write!(self.writer(), "}}");
            }
        }
    }
}
