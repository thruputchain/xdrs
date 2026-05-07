#![allow(dead_code)]

use xdrs::xdr_repr::*;
use xdrs::*;

pub mod optional_boxed_self {
    use super::*;

    #[derive(XDR)]
    struct Node {
        value: u32,
        next: Option<Box<Node>>,
    }

    check_format! { optional_boxed_self_reference_renders_finitely
        Node::def()
        =>
            "struct Node {"
            "\tunsigned int value;"
            "\tNode *next;"
            "};"
    }
}

pub mod owned_vec_self {
    use super::*;

    #[derive(XDR)]
    struct RecursiveVecNode {
        children: Vec<RecursiveVecNode>,
    }

    check_format! { owned_vec_self_reference_renders_finitely
        RecursiveVecNode::def()
        =>
            "struct RecursiveVecNode {"
            "\tRecursiveVecNode children<>;"
            "};"
    }
}

pub mod fixed_array_of_boxed_self {
    use super::*;

    #[derive(XDR)]
    struct ArrayNode {
        children: [Box<ArrayNode>; 2],
    }

    check_format! { fixed_array_of_boxed_self_reference_renders_finitely
        ArrayNode::def()
        =>
            "struct ArrayNode {"
            "\tArrayNode children[2];"
            "};"
    }
}

pub mod bounded_vec_self {
    use super::*;

    #[derive(XDR)]
    struct BoundedVecNode {
        children: VecM<BoundedVecNode, 3>,
    }

    check_format! { bounded_vec_self_reference_renders_finitely
        BoundedVecNode::def()
        =>
            "struct BoundedVecNode {"
            "\tBoundedVecNode children<3>;"
            "};"
    }
}

pub mod nested_optional_vec_self {
    use super::*;

    #[derive(XDR)]
    struct NestedOptionalVecNode {
        nested: Vec<Option<Box<NestedOptionalVecNode>>>,
    }

    check_format! { nested_optional_vec_self_reference_renders_finitely
        NestedOptionalVecNode::def()
        =>
            "struct NestedOptionalVecNode {"
            "\tstruct {"
            "\t\tNestedOptionalVecNode *unnamed_0;"
            "\t} nested<>;"
            "};"
    }
}

pub mod result_boxed_self {
    use super::*;

    #[derive(XDR)]
    struct ResultNode {
        payload: Result<Box<ResultNode>, ()>,
    }

    check_format! { result_boxed_self_payload_renders_finitely
        ResultNode::def()
        =>
            "struct ResultNode {"
            "\tunion switch (int which)"
            "\t{"
            "\tcase 0:"
            "\t\tResultNode ok;"
            "\tcase 1:"
            "\t\tvoid;"
            "\t} payload;"
            "};"
    }
}
