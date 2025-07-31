#[macro_export]
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

#[test]
fn test_lines() {
    let expected = "one\ntwo\n";
    let actual = lines!["one" "two"];
    assert_eq!(actual, expected);
}

#[macro_export]
macro_rules! assert_formatted {
    ($value:expr, $expected:expr) => {
        {
            let value = $value;
            let expected = $expected;
            let actual = format(&value);

            if actual != expected {
                panic!("unexpected formatting:\n\n\
                        /* ---- expected ---- */\n\
                        {expected}<END>\n\
                        /* ----- actual ----- */\n\
                        {actual}<END>\n\
                        /* ------------------ */\n");
            }
        }
    }
}

#[macro_export]
macro_rules! check_format {
    { $test_name:ident $val:expr => $($ln:expr $(,)?)* } => {
        #[test]
        fn $test_name() {
            crate::assert_formatted!($val, crate::lines![$($ln),*]);
        }
    };
    { #[$attr:meta] $test_name:ident $val:expr => $($ln:expr $(,)?)* } => {
        #[test]
        #[$attr]
        fn $test_name() {
            crate::assert_formatted!($val, crate::lines![$($ln),*]);
        }
    };
}
