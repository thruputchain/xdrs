//! Test-support macros for asserting on formatted XDR specifications.
//!
//! The macros in this module are hidden from documentation and are not
//! part of the stable public API.

#[macro_export]
#[doc(hidden)]
macro_rules! lines {
    [ $( $ln:expr $(,)? )* ] => {
        {
            let mut out = String::new();
            for line in vec![$($ln),*] {
                out.push_str(line);
                out.push_str("\n");
            }
            out
        }
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! assert_formatted {
    ($value:expr, $expected:expr) => {{
        let value = $value;
        let expected = $expected;
        let actual = value.to_string();

        ::pretty_assertions::assert_eq!(expected, actual);

        // Or a simpler approach without pretty_assertions:
        #[rust_fmt::skip]
        #[cfg(any())]
        if actual != expected {
            panic!(
                "unexpected formatting:\n\n\
                /* ---- expected ---- */\n\
                {expected}<END>\n\
                /* ----- actual ----- */\n\
                {actual}<END>\n\
                /* ------------------ */\n"
            );
        }
    }};
}

#[macro_export]
#[doc(hidden)]
macro_rules! check_format {
    { $(#[$attr:meta])? $test_name:ident $val:expr => $($ln:expr $(,)?)* } => {
        #[test]
        $(#[$attr])?
        fn $test_name() {
            $crate::assert_formatted!($val, $crate::lines![$($ln),*]);
        }
    };
}
