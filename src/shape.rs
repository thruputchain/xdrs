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
    extern crate std;

    use super::*;
    use util::*;

    #[test]
    #[should_panic(expected = "No ink")]
    pub fn perform_tour() {
        let host = host::Person::new("Dave", 42);
        let mut visitor = visitor::Printer::new();

        host.accept(&mut visitor).unwrap_or_else(|b|
            panic!("Visit failed: {}", b));

        std::println!("Dave saw: {}", visitor.into_story());
    }

    mod host {
        use super::*;

        // Person: Host
        pub struct Person {
            title: &'static str,
            age: u32,
        }

        impl Person {
            pub fn new(title: &'static str, age: u32) -> Self {
                Self { title, age }
            }
        }

        impl Host for Person {
            fn accept<V>(&self, v: &mut V) -> Result<(), V::Break>
                where
                    V: Visitor
            {
                continue_ok((|| -> ControlFlow<V::Break> {
                    let mut s = v.visit_struct("Person", 2)?;

                    s.visit_field("title", &self.title)?;
                    s.visit_field("age", &self.age)?;

                    ControlFlow::Continue(())
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

            pub fn into_story(self) -> Story {
                self.story
            }
        }

        impl Visitor for Printer {
            type Break = PrintError;
            type VisitStruct = PrintStruct;

            fn visit_struct(&mut self, name: &str, n_fields: u32) -> PrintFlow<PrintStruct> {
                no_ink()
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
