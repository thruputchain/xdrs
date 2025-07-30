use xdr_repr::rpc::*;
use xdr_repr::xdr::TypeSpec;

#[test]
fn show_prog() {
    let proc1 = Procedure {
        id: Id("ping".into()),
        value: Constant(1),
        return_type: None,
        arg_types: vec![
            TypeSpec::Int,
        ],
    };

    let proc2 = Procedure {
        id: Id("pong".into()),
        value: Constant(2),
        return_type: Some(TypeSpec::UnsignedInt),
        arg_types: vec![],
    };

    let version10 = Version {
        id: Id("v0.1.0".into()),
        value: Constant(10),
        procedures: vec![proc1, proc2],
    };

    let program = Program {
        id: Id("PingPong".into()),
        value: Constant(42),
        versions: vec![version10],
    };

    let expected =
"program PingPong {
	version v0.1.0 {
		void ping (
			int
		) = 1;
		unsigned int pong (
		) = 2;
	} = 10;
} = 42;
";

    let output = program.to_string();
    assert_eq!(output, expected);
}
