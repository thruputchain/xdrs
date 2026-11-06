//! The parser's handling of comments, declarations, numbers and errors.

use xdr_codegen::ast::*;
use xdr_codegen::diag::{Diagnostic, LineIndex, Severity};

fn parse(src: &str) -> xdr_codegen::Parsed {
    xdr_codegen::parse(src).unwrap_or_else(|diags| panic!("{}", render(src, &diags)))
}

fn errors(src: &str) -> String {
    match xdr_codegen::parse(src) {
        Ok(_) => panic!("parsed"),
        Err(diags) => render(src, &diags),
    }
}

fn render(src: &str, diags: &[Diagnostic]) -> String {
    let lines = LineIndex::new(src);
    diags
        .iter()
        .map(|d| format!("{}\n", d.display("t.x", &lines)))
        .collect()
}

fn warnings(src: &str) -> String {
    render(src, &parse(src).warnings)
}

fn items(src: &str) -> Vec<Item> {
    parse(src).spec.items
}

fn only(src: &str) -> Item {
    let mut items = items(src);
    assert_eq!(items.len(), 1, "{items:#?}");
    items.pop().unwrap()
}

fn struct_body(item: &Item) -> &StructBody {
    match &item.kind {
        ItemKind::Struct(s) => &s.body,
        kind => panic!("{kind:?}"),
    }
}

fn union_body(item: &Item) -> &UnionBody {
    match &item.kind {
        ItemKind::Union(u) => &u.body,
        kind => panic!("{kind:?}"),
    }
}

fn enum_body(item: &Item) -> &EnumBody {
    match &item.kind {
        ItemKind::Enum(e) => &e.body,
        kind => panic!("{kind:?}"),
    }
}

fn decl(member: &Member) -> &Declaration {
    member.decl.as_ref().expect("not void")
}

fn name(v: &Value) -> &str {
    match v {
        Value::Ident(i) => &i.name,
        Value::Number(n) => &n.text,
    }
}

// ---- comments -------------------------------------------------------------

#[test]
fn header_is_set_off_by_a_blank_line() {
    let spec = parse("// Copyright\n// License\n\n// About S.\nstruct S { int a; };\n").spec;
    assert_eq!(spec.header, "// Copyright\n// License\n");
    assert_eq!(spec.items[0].comment, "// About S.\n");
}

#[test]
fn header_takes_every_block_set_off_before_the_first_token() {
    let spec = parse("/* one */\n\n// two\n\n// three\nconst A = 1;\n").spec;
    assert_eq!(spec.header, "/* one */\n\n// two\n");
    assert_eq!(spec.items[0].comment, "// three\n");
}

#[test]
fn no_header_without_a_blank_line() {
    let spec = parse("// About S.\nstruct S { int a; };\n").spec;
    assert_eq!(spec.header, "");
    assert_eq!(spec.items[0].comment, "// About S.\n");
}

#[test]
fn blank_line_before_a_definition_is_kept() {
    let spec = parse("const A = 1;\n// About B.\n\nconst B = 2;\n").spec;
    assert_eq!(spec.items[1].comment, "// About B.\n\n");
}

#[test]
fn typedef_takes_the_trailing_comment_after_its_semicolon() {
    let item = only("typedef unsigned int Protocol; // #[rust(newtype)]\n");
    // Indented to the comment's column, as nothing leads it.
    assert_eq!(
        item.comment,
        format!("{}// #[rust(newtype)]\n", " ".repeat(31))
    );

    let item = only("// Doc.\ntypedef int T; // marker\n");
    assert_eq!(item.comment, "// Doc.\n// marker\n");
}

#[test]
fn const_takes_leading_and_trailing_comments() {
    let item = only("// Doc.\nconst A = 1; // more\n");
    assert_eq!(item.comment, "// Doc.\n// more\n");
}

#[test]
fn struct_takes_only_its_leading_comment() {
    let item = only("// Doc.\nstruct S { int a; }; // dropped\n");
    assert_eq!(item.comment, "// Doc.\n");
}

#[test]
fn field_comments() {
    let item = only(
        "struct S {\n  // Leads a.\n  int a; // trails a\n  /* Leads b. */\n  opaque b<>;\n  void;\n};\n",
    );
    let fields = &struct_body(&item).fields;
    assert_eq!(fields[0].comment, "  // Leads a.\n// trails a\n");
    assert_eq!(fields[1].comment, "  /* Leads b. */\n");
    assert_eq!(fields[2].comment, "");
    assert!(fields[2].decl.is_none());
}

#[test]
fn trailing_comment_continues_onto_following_comment_lines() {
    let item = only("struct S {\n  int a; // one\n         // two\n  /// three\n  int b;\n};\n");
    let fields = &struct_body(&item).fields;
    assert_eq!(fields[0].comment, "         // one\n         // two\n");
    // A `///` line ends the trailing comment, and leads what follows.
    assert_eq!(fields[1].comment, "  /// three\n");
}

#[test]
fn comment_mid_line_trails_the_previous_token() {
    // `/* x */` shares its line with code, so it trails the `;` before it,
    // which ends `a`.
    let item = only("struct S {\n  int a;\n/* x */ int b;\n};\n");
    let fields = &struct_body(&item).fields;
    assert_eq!(fields[0].comment, "/* x */\n");
    assert_eq!(fields[1].comment, "");
}

#[test]
fn enum_member_comments() {
    let item = only("enum E {\n  // Leads A.\n  A = 0, // trails A\n  B = 1 // trails B\n};\n");
    let members = &enum_body(&item).members;
    assert_eq!(members[0].comment, "  // Leads A.\n// trails A\n");
    assert_eq!(members[1].comment, "        // trails B\n");
}

#[test]
fn union_arm_comments() {
    let item = only(
        "union U switch (int k) {\n  case 0:\n    // Leads x.\n    int x; // trails x\n  default:\n    void; // nothing\n};\n",
    );
    let arms = &union_body(&item).arms;
    assert_eq!(arms[0].member.comment, "    // Leads x.\n// trails x\n");
    assert_eq!(arms[1].member.comment, "          // nothing\n");
}

// ---- definitions ----------------------------------------------------------

#[test]
fn declarations() {
    let item = only(
        "struct S {\n  int a;\n  Foo *b;\n  unsigned hyper c[3];\n  Bar d<>;\n  Bar e<MAX>;\n  opaque f[32];\n  opaque g<16>;\n  string h<>;\n};\n",
    );
    let fields = &struct_body(&item).fields;
    let tys: Vec<_> = fields.iter().map(|f| &decl(f).ty).collect();
    assert_eq!(tys[0], &DeclType::Plain(TypeSpec::Int));
    assert!(matches!(tys[1], DeclType::Optional(TypeSpec::Named(i)) if i.name == "Foo"));
    assert!(
        matches!(tys[2], DeclType::Array { elem: TypeSpec::UnsignedHyper, len } if name(len) == "3")
    );
    assert!(matches!(tys[3], DeclType::VarArray { max: None, .. }));
    assert!(matches!(tys[4], DeclType::VarArray { max: Some(m), .. } if name(m) == "MAX"));
    assert!(matches!(tys[5], DeclType::Opaque { len } if name(len) == "32"));
    assert!(matches!(tys[6], DeclType::VarOpaque { max: Some(m) } if name(m) == "16"));
    assert!(matches!(tys[7], DeclType::String { max: None }));
    let names: Vec<_> = fields.iter().map(|f| decl(f).name.name.as_str()).collect();
    assert_eq!(names, ["a", "b", "c", "d", "e", "f", "g", "h"]);
}

#[test]
fn inline_types() {
    let item = only(
        "struct S {\n  enum { A = 0 } e;\n  struct { int x; } s;\n  union switch (bool b) { case TRUE: int y; case FALSE: void; } u;\n};\n",
    );
    let fields = &struct_body(&item).fields;
    assert!(matches!(
        &decl(&fields[0]).ty,
        DeclType::Plain(TypeSpec::Enum(_))
    ));
    assert!(matches!(
        &decl(&fields[1]).ty,
        DeclType::Plain(TypeSpec::Struct(_))
    ));
    let DeclType::Plain(TypeSpec::Union(u)) = &decl(&fields[2]).ty else {
        panic!()
    };
    assert_eq!(u.switch_type, TypeSpec::Bool);
    assert_eq!(name(&u.arms[0].cases[0]), "TRUE");
}

#[test]
fn union_arms() {
    let item = only(
        "union U switch (Kind which) {\n  case A:\n  case B:\n    int x;\n  case C:\n    void;\n  default:\n    hyper y;\n};\n",
    );
    let u = union_body(&item);
    assert!(matches!(&u.switch_type, TypeSpec::Named(i) if i.name == "Kind"));
    assert_eq!(u.switch_name.name, "which");
    let cases: Vec<Vec<&str>> = u
        .arms
        .iter()
        .map(|a| a.cases.iter().map(name).collect())
        .collect();
    assert_eq!(cases, [vec!["A", "B"], vec!["C"], vec![]]);
    assert_eq!(
        u.arms.iter().map(|a| a.is_default).collect::<Vec<_>>(),
        [false, false, true]
    );
}

#[test]
fn default_arm_must_be_last() {
    assert_eq!(
        errors("union U switch (int k) {\n  default: void;\n  case 1: int x;\n};\n"),
        "t.x:3:3: error: expected `}`, found `case`\n",
    );
}

#[test]
fn union_needs_an_arm() {
    assert_eq!(
        errors("union U switch (int k) {\n};\n"),
        "t.x:2:1: error: expected `case` or `default`, found `}`\n",
    );
}

#[test]
fn programs() {
    let item = only(
        "program P {\n  version V1 {\n    void ping(void) = 0;\n    // Adds.\n    Sum add(int, unsigned hyper) = 1; // more\n  } = 1;\n} = 0x20000001;\n",
    );
    let ItemKind::Program(p) = &item.kind else {
        panic!()
    };
    assert_eq!(p.name.name, "P");
    assert!(matches!(&p.value, Value::Number(n) if n.value == 0x2000_0001));
    let procs = &p.versions[0].procs;
    assert_eq!(procs[0].result, None);
    assert!(procs[0].args.is_empty());
    assert!(matches!(&procs[1].result, Some(TypeSpec::Named(i)) if i.name == "Sum"));
    assert_eq!(procs[1].args, [TypeSpec::Int, TypeSpec::UnsignedHyper]);
    assert_eq!(procs[1].comment, "    // Adds.\n// more\n");
}

#[test]
fn namespaces_nest() {
    let items =
        items("namespace a {\n  const X = 1;\n  namespace b { const Y = 2; }\n}\nconst Z = 3;\n");
    assert_eq!(items.len(), 2);
    let ItemKind::Namespace(a) = &items[0].kind else {
        panic!()
    };
    assert_eq!(a.name.name, "a");
    assert!(matches!(&a.body[1].kind, ItemKind::Namespace(b) if b.body.len() == 1));
}

#[test]
fn literals() {
    let items = items("%#include \"a.hh\"\n%#include \"b.hh\"\nconst A = 1;\n%// after\n");
    let kinds: Vec<_> = items.iter().map(|i| &i.kind).collect();
    assert_eq!(
        kinds[0],
        &ItemKind::Literal("#include \"a.hh\"\n#include \"b.hh\"\n".into())
    );
    assert!(matches!(kinds[1], ItemKind::Const(_)));
    assert_eq!(kinds[2], &ItemKind::Literal("// after\n".into()));
}

#[test]
fn literal_inside_a_definition_goes_before_it() {
    let items = items("struct S {\n%// inside\n  int a;\n};\n");
    assert_eq!(items[0].kind, ItemKind::Literal("// inside\n".into()));
    assert!(matches!(items[1].kind, ItemKind::Struct(_)));
}

#[test]
fn literal_before_a_namespace_stays_outside_it() {
    let items = items("%// outside\nnamespace n { const A = 1; }\n");
    assert_eq!(items[0].kind, ItemKind::Literal("// outside\n".into()));
    let ItemKind::Namespace(n) = &items[1].kind else {
        panic!()
    };
    assert_eq!(n.body.len(), 1);
}

#[test]
fn keyword_prefix_is_an_identifier() {
    let item = only("const constant = 1;\n");
    let ItemKind::Const(c) = &item.kind else {
        panic!()
    };
    assert_eq!(c.name.name, "constant");
}

// ---- numbers --------------------------------------------------------------

fn number(text: &str) -> i128 {
    let item = only(&format!("const A = {text};\n"));
    let ItemKind::Const(c) = item.kind else {
        panic!()
    };
    assert_eq!(c.value.text, text);
    c.value.value
}

#[test]
fn numbers() {
    assert_eq!(number("0"), 0);
    assert_eq!(number("42"), 42);
    assert_eq!(number("-1"), -1);
    assert_eq!(number("+7"), 7);
    assert_eq!(number("1'000'000"), 1_000_000);
    assert_eq!(number("1_000"), 1000);
    assert_eq!(number("0xABCD'EF'01"), 0xABCD_EF01);
    assert_eq!(number("0x80000000"), 0x8000_0000);
    assert_eq!(number("62'500'000'000"), 62_500_000_000);
    assert_eq!(number("0xffffffffffffffff"), i128::from(u64::MAX));
    assert_eq!(number("-9223372036854775808"), i128::from(i64::MIN));
    // A leading zero makes it octal, as in RFC 4506 and C++.
    assert_eq!(number("010"), 8);
}

#[test]
fn bad_numbers() {
    assert_eq!(
        errors("const A = 0x1'0000'0000'0000'0000;\n"),
        "t.x:1:11: error: `0x1'0000'0000'0000'0000` is not a valid number, or does not fit 64 bits\n",
    );
    assert_eq!(
        errors("const A = 08;\n"),
        "t.x:1:11: error: `08` is not a valid number, or does not fit 64 bits\n",
    );
}

// ---- diagnostics ----------------------------------------------------------

#[test]
fn warnings_for_rfc_deviations() {
    assert_eq!(
        warnings("enum E { A = 0, };\n"),
        "t.x:1:15: warning: RFC 4506 does not allow a trailing comma\n",
    );
    assert_eq!(
        warnings("typedef unsigned T;\n"),
        "t.x:1:9: warning: RFC 4506 requires `int` or `hyper` after `unsigned` (assuming `unsigned int`)\n",
    );
    assert_eq!(
        warnings("union U switch (bool b) { case true: void; };\n"),
        "t.x:1:32: warning: RFC 4506 spells the boolean constants TRUE and FALSE\n",
    );
    assert_eq!(
        warnings("union U switch (int k) { case 1: default: void; };\n"),
        "t.x:1:34: warning: RFC 4506 does not allow case labels on the default arm\n",
    );
    assert_eq!(
        warnings("typedef uint32_t T;\n"),
        "t.x:1:9: warning: use `unsigned int` in XDR source to get `uint32_t`\n",
    );
}

#[test]
fn warnings_do_not_fail() {
    let parsed = parse("enum E { A = 0, };\n");
    assert_eq!(parsed.warnings[0].severity, Severity::Warning);
    assert_eq!(parsed.spec.items.len(), 1);
}

#[test]
fn syntax_errors() {
    assert_eq!(
        errors("struct S { int a };\n"),
        "t.x:1:18: error: expected `;`, found `}`\n",
    );
    assert_eq!(
        errors("typedef void T;\n"),
        "t.x:1:9: error: expected a type, found `void`\n",
    );
    assert_eq!(
        errors("int x;\n"),
        "t.x:1:1: error: expected a definition, found `int`\n",
    );
    assert_eq!(
        errors("const A = B;\n"),
        "t.x:1:11: error: expected a number, found `B`\n",
    );
    assert_eq!(
        errors("struct S { int a; }\n"),
        "t.x:2:1: error: expected `;`, found the end of the file\n",
    );
}

#[test]
fn lexical_errors_are_all_reported() {
    assert_eq!(
        errors("const A = 1;\nconst $B = 2;\nchar c;\n"),
        "t.x:2:7: error: unexpected `$B`\n\
         t.x:2:10: error: expected an identifier, found `=`\n\
         t.x:3:1: error: `char` is reserved and cannot be used\n",
    );
}

#[test]
fn duplicate_type_names() {
    assert_eq!(
        errors("struct S { int a; };\nnamespace n { typedef int S; }\n"),
        "t.x:2:15: error: duplicate declaration of typedef `S`\n",
    );
}

#[test]
fn columns_count_tabs_to_eight() {
    assert_eq!(
        errors("struct S {\n\tint a\n};\n"),
        "t.x:3:1: error: expected `;`, found `}`\n",
    );
    assert_eq!(
        errors("struct S {\n\tint a b;\n};\n"),
        "t.x:2:15: error: expected `;`, found `b`\n",
    );
}
