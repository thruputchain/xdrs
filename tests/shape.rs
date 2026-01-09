use xdrs::shape::*;

// Create a `Person` host-object and ask it to accept a `Printer` visitor, capturing the
// result.
#[test]
pub fn perform_tour() {
    let host = host::Person::new("Dave", 42);

    let visitor = visitor::Printer::new();

    let result = host
        .accept(visitor)
        .unwrap_or_else(|b| panic!("Visit failed: {}", b));

    assert_eq!(result, "Person { title: \"Dave\", age: 42 }");
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

    impl Host for Person {
        // Guide the visitor to each XDR component of a `Person` value
        fn guide<V>(&self, v: &mut V) -> Result<(), V::Error>
        where
            V: Visitor
        {
            let mut s = v.visit_struct("Person", 2)?;

            s.visit_field("title", self.title)?;
            s.visit_field("age", self.age)?;

            Ok(())
        }
    }
}

mod visitor {
    use super::*;
    use core::fmt::{self, Write};
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
        // depth: u32,
    }

    impl Printer {
        pub fn new() -> Self {
            Self {
                story: String::new(),
                // depth: 0,
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

        fn visit_string(&mut self, s: &str) -> PrintResult {
            write!(self.writer(), "{:?}", s)?;

            Ok(())
        }

        fn visit_u32(&mut self, n: u32) -> PrintResult {
            write!(self.writer(), "{}", n)?;

            Ok(())
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
        fn visit_field<T: Host>(&mut self, name: &str, value: T) -> PrintResult {
            let index = self.n_fields_visited;
            let n_fields = self.n_fields;

            assert!(index < n_fields, "Too many fields");

            write!(self.writer(), "{}: ", name)?;

            value.guide(self.printer)?;

            let sep = if index < n_fields - 1 { ", " } else { " " };
            self.writer().write_str(sep)?;

            self.n_fields_visited += 1;

            Ok(())
        }
    }

    impl core::ops::Drop for PrintStruct<'_> {
        fn drop(&mut self) {
            write!(self.writer(), "}}").unwrap_or_else(|e|
                panic!("Failed closing a struct: {}", e));
        }
    }
}
