use xdr_repr::rpc::*;
use xdr_repr::xdr::TypeSpec;

#[test]
fn show_prog() {
    let proc1 = Procedure {
        id: Id::new("ping"),
        value: Constant(1),
        return_type: None,
        arg_types: vec![
            Arg {
                ty: TypeSpec::Int,
                name: "num".to_owned(),
            },
            Arg {
                ty: TypeSpec::Bool,
                name: "okay".to_owned(),
            },
        ],
    };

    let proc2 = Procedure {
        id: Id::new("pong"),
        value: Constant(2),
        return_type: Some(TypeSpec::Unsigned),
        arg_types: vec![],
    };

    let version10 = Version {
        id: Id::new("v0_1_0"),
        value: Constant(10),
        procedures: vec![proc1, proc2],
    };

    let program = Program {
        id: Id::new("PingPong"),
        value: Constant(42),
        versions: vec![version10],
    };

    let expected = "program PingPong {
\tversion v0_1_0 {
\t\tvoid ping (
\t\t\tint /* num */,
\t\t\tbool /* okay */
\t\t) = 1;
\t\tunsigned int pong (
\t\t) = 2;
\t} = 10;
} = 42;
";

    let output = program.to_string();
    assert_eq!(output, expected);
}

#[test]
#[should_panic(expected = "Invalid XDR identifier")]
fn rpc_id_rejects_invalid_identifier() {
    Id::new("Bad-Program");
}
