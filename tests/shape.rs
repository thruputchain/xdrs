use xdrs::shape::*;

// Create a `Person` host-object and ask it to accept a `Printer` visitor, capturing the
// result.
#[test]
pub fn perform_tour() {
    let host = host::make_dave();

    let visitor = visitor::Printer::new();

    let result = host
        .accept(visitor)
        .unwrap_or_else(|b| panic!("Visit failed: {}", b));

    assert_eq!(result,
        "Person { \
            title: \"Dave\", \
            age: 42, \
            children: [\
                Person { title: \"Alicia\", age: 9, children: [] }, \
                Person { title: \"Devon\", age: 13, children: [] }\
            ] \
        }");
}

mod host {
    use super::*;

    // The type whose XDR representation will be visited
    pub struct Person {
        title: &'static str,
        age: u32,
        children: Vec<Person>,
    }

    impl Person {
        pub const fn new(title: &'static str, age: u32, children: Vec<Person>) -> Self {
            Self { title, age, children }
        }
    }

    pub fn make_dave() -> Person {
        let kids = vec![Person::new("Alicia", 9, vec!()), Person::new("Devon", 13, vec!())];
        Person::new("Dave", 42, kids)
    }

    impl Host for Person {
        // Guide the visitor to each XDR component of a `Person` value
        fn guide<V>(&self, v: &mut V) -> Result<(), V::Error>
        where
            V: Visitor
        {
            let mut s = v.visit_struct("Person", 3)?;

            s.visit_field("title", &self.title)?;
            s.visit_field("age", &self.age)?;
            s.visit_field("children", &self.children)?;

            s.finish()?;

            Ok(())
        }
    }
}

mod visitor {
    use super::*;
    use core::fmt::{self, Write};
    use heapless::String;
    use thiserror::Error;
    use core::marker::PhantomData;

    #[derive(Debug, Error)]
    pub enum PrintError {
        #[error("Write error: {0}")]
        Write(#[from] fmt::Error),

        #[error("Array too long")]
        ArrayTooLong,
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

        fn writer(&mut self) -> &mut impl Write {
            &mut self.story
        }
    }

    impl Visitor for Printer {
        type Error = PrintError;
        type VisitStruct<'a> = PrintStruct<'a>;
        type VisitArray<'a, T> = PrintArray<'a, T>;
        type Return = Story;

        fn into_return(self) -> Story {
            self.story
        }

        fn visit_string(&mut self, s: &str) -> PrintResult {
            write!(self.writer(), "{:?}", s)?;

            Ok(())
        }

        fn visit_u32(&mut self, n: u32) -> PrintResult {
            write!(self.writer(), "{}", n)?;

            Ok(())
        }

        fn visit_struct<'a>(
            &'a mut self,
            name: &str,
            n_fields: u32,
        ) -> PrintResult<PrintStruct<'a>> {
            write!(self.writer(), "{name} {{ ")?;

            Ok(PrintStruct::new(n_fields, self))
        }

        fn visit_array<'v, T: Host>(
            &'v mut self,
            n_elts: usize,
        ) -> PrintResult<PrintArray<'v, T>> {
            write!(self.writer(), "[")?;

            let len = n_elts.try_into().map_err(|_| PrintError::ArrayTooLong)?;

            Ok(PrintArray::new(len, self))
        }
    }

    pub struct PrintArray<'a, T> {
        n_elts: u32,
        n_elts_visited: u32,
        printer: &'a mut Printer,
        finished: bool,
        _marker: PhantomData<T>,
    }

    impl<'a, T> PrintArray<'a, T> {
        pub fn new(n_elts: u32, printer: &'a mut Printer) -> Self {
            Self {
                n_elts,
                n_elts_visited: 0,
                printer,
                finished: false,
                _marker: PhantomData,
            }
        }

        fn writer(&mut self) -> &mut impl Write {
            self.printer.writer()
        }
    }

    impl<T> VisitorArray<T, PrintError> for PrintArray<'_, T> {
        fn visit_elt(&mut self, value: &T) -> PrintResult
        where T: Host
        {
            let index = self.n_elts_visited;
            let n_elts = self.n_elts;

            assert!(index < n_elts, "Too many elements");

            value.guide(self.printer)?;

            let sep = if index < n_elts - 1 { ", " } else { "" };
            self.writer().write_str(sep)?;

            self.n_elts_visited += 1;

            Ok(())
        }

        fn finish(mut self) -> PrintResult {
            assert!(self.n_elts_visited == self.n_elts);

            write!(self.writer(), "]")?;

            self.finished = true;

            Ok(())
        }
    }

    impl<T> core::ops::Drop for PrintArray<'_, T> {
        fn drop(&mut self) {
            if !self.finished {
                panic!("Forgot to close an array")
            }
        }
    }

    pub struct PrintStruct<'a> {
        n_fields: u32,
        n_fields_visited: u32,
        printer: &'a mut Printer,
        finished: bool,
    }

    impl<'a> PrintStruct<'a> {
        pub fn new(n_fields: u32, printer: &'a mut Printer) -> Self {
            Self {
                n_fields,
                n_fields_visited: 0,
                printer,
                finished: false,
            }
        }

        pub fn writer(&mut self) -> &mut impl Write {
            self.printer.writer()
        }
    }

    impl VisitorStruct<PrintError> for PrintStruct<'_> {
        fn visit_field<T: Host>(&mut self, name: &str, value: &T) -> PrintResult {
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

        fn finish(mut self) -> PrintResult {
            assert!(self.n_fields_visited == self.n_fields);

            write!(self.writer(), "}}")?;

            self.finished = true;

            Ok(())
        }
    }

    impl core::ops::Drop for PrintStruct<'_> {
        fn drop(&mut self) {
            if !self.finished {
                panic!("Forgot to close a struct")
            }
        }
    }
}
