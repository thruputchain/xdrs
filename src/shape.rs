//!
//! Traits [`Host`] and [`Visitor`] for guiding a visitor
//! through the structure of an XDR value.
//!

#![allow(unused)]

use core::ops::ControlFlow;

/// A type that wants to visit a host
pub trait Visitor {
    /// A type the `Visitor` returns when it wants to stop
    type Break;

    /// A type that will visit the parts of a struct
    type VisitStruct: StructVisitor<Self::Break>;

    /// The final result of a visit
    type Return;

    /// Visit an XDR structure
    fn visit_struct(&mut self, name: &str, n_fields: u32) ->
        ControlFlow<Self::Break, Self::VisitStruct>;

    /// Yield the final result, consuming the `Visitor`
    fn into_return(self) -> Self::Return;
}

/// A type that wants to visit an XDR structure's fields
pub trait StructVisitor<B> {
    /// Visit a field of an XDR structure
    fn visit_field<T>(&mut self, name: &str, value: T) -> ControlFlow<B>;
}

/// A type that accepts visitors
pub trait Host {
    /// Accept a [`Visitor`], and guide it around the [`Host`]'s establishment.
    ///
    /// Return the visitor's final result if the tour went well.
    ///
    /// Return `Err(b)` if the visitor pleads for a `ControlFlow::Break(b)`.
    ///
    fn accept<V>(&self, v: V) -> Result<V::Return, V::Break>
        where
            V: Visitor;
}

#[cfg(test)]
mod test {
    extern crate std;

    use super::*;
    use util::*;

    #[test]
    #[should_panic(expected = "No ink")]
    pub fn perform_tour() {
        let host = host::Person::new("Dave", 42);

        let visitor = visitor::Printer::new();

        let result = host.accept(visitor).unwrap_or_else(|b|
            panic!("Visit failed: {}", b));

        std::println!("Dave's review: {}", result);
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
        }

        // Accept a visitor and guide it to each XDR component of a `Person` value
        impl Host for Person {
            fn accept<V>(&self, mut v: V) -> Result<V::Return, V::Break>
                where
                    V: Visitor
            {
                continue_ok((|| -> ControlFlow<V::Break, V::Return> {
                    let mut s = v.visit_struct("Person", 2)?;

                    s.visit_field("title", &self.title)?;
                    s.visit_field("age", &self.age)?;

                    ControlFlow::Continue(v.into_return())
                })())
            }
        }
    }

    mod visitor {
        use super::*;
        use heapless::String;
        use thiserror::Error;

        #[derive(Debug, Error)]
        pub enum PrintError {
            #[error("No ink remaining.")]
            NoInk,
        }

        type PrintFlow<T = ()> = ControlFlow<PrintError, T>;

        type Story = String<1000>;

        pub struct Printer {
            story: Story,
        }

        impl Printer {
            pub fn new() -> Self {
                Self { story: String::new() }
            }
        }

        impl Visitor for Printer {
            type Break = PrintError;
            type VisitStruct = PrintStruct;
            type Return = Story;

            fn visit_struct(&mut self, name: &str, n_fields: u32) -> PrintFlow<PrintStruct> {
                no_ink()
            }

            fn into_return(self) -> Story {
                self.story
            }
        }

        pub struct PrintStruct {
            n_fields: u32,
        }

        impl StructVisitor<PrintError> for PrintStruct {
            fn visit_field<T>(&mut self, name: &str, value: T) -> PrintFlow {
                no_ink()
            }
        }

        fn no_ink<T>() -> PrintFlow<T> {
            ControlFlow::Break(PrintError::NoInk)
        }

        fn continue_with<T>(x: T) -> PrintFlow<T> {
            ControlFlow::Continue(x)
        }

        fn please_continue() -> PrintFlow<()> {
            PrintFlow::Continue(())
        }
    }

    mod util {
        use super::*;

        pub fn continue_ok<B, C>(cf: ControlFlow<B, C>) -> Result<C, B> {
            match cf {
                ControlFlow::Continue(c) => Ok(c),
                ControlFlow::Break(b) => Err(b),
            }
        }
    }
}
