const KEYWORDS: [&str; 18] = [
   "bool", "case", "const", "default", "double", "quadruple", "enum",
   "float", "hyper", "int", "opaque", "string", "struct", "switch",
   "typedef", "union", "unsigned", "void"
];

pub fn is_keyword<S: AsRef<str>>(s: S) -> bool {
    KEYWORDS.into_iter().any(|k| s.as_ref() == k)
}
