mod common;
use common::*;
// use xdrs::{ReadXDR, ReadStructuredXDR, ReadResult, Reader, WriteXDR, read_xdr};
use xdrs::*;
use xdrs::serialize::reader::*;

use std::vec::Vec;
use std::vec;

#[derive(Clone, Debug, PartialEq)]
struct Path(Vec<usize>);

impl Path {
    fn new() -> Self {
        Self(vec![])
    }
}

impl Path {
    fn child(&self, i: usize) -> Self {
        let mut p = self.clone();
        p.0.push(i);
        p
    }

    fn depth(&self) -> usize {
        self.0.len()
    }
}

impl Into<()> for &Path {
    fn into(self) -> () {
        ()
    }
}

impl Structured for Path {
    fn seq(&mut self, _len: Option<usize>) -> Result<()> {
        self.0.push(0);
        Ok(())
    }

    fn next(&mut self) -> Result<()> {
        let i = self.0.len();
        self.0[i-1] += 1;
        Ok(())
    }

    fn end(&mut self) -> Result<()> {
        self.0.pop();
        Ok(())
    }
}

use xdr_common::Box;

#[derive(Debug, PartialEq, WriteXDR)]
struct Node {
    #[xdrs(as_type = "()")]
    path: Path,
    left: Option<Box<Node>>,
    right: Option<Box<Node>>,
}

impl Node {
    pub fn write<W: std::fmt::Write>(&self, w: &mut W) -> Result<(), std::fmt::Error> {
        self.write_depth(w, 0)
    }

    fn write_depth<W: std::fmt::Write>(&self, w: &mut W, depth: usize) -> Result<(), std::fmt::Error> {
        writeln!(w, "{:indent$}{:?}", "", self.path, indent=depth)?;

        if let Some(left) = self.left.as_ref() {
            left.as_ref().write_depth(w, depth + 1)?;
        }
        if let Some(right) = self.right.as_ref() {
            right.as_ref().write_depth(w, depth + 1)?;
        }

        Ok(())
    }
}

fn mktree(path: &Path) -> Option<Box<Node>> {
    const MAX_DEPTH: usize = 5;

    if path.depth() >= MAX_DEPTH {
        None
    } else {
        Some(Box::new(Node {
            path: path.clone(),
            left: mktree(&path.child(0)),
            right: mktree(&path.child(1)),
        }))
    }
}

// Manually implement reading in the same way that will be derived
impl<'r> ReadStructuredXDR<'r, Path> for Node {
    fn read_structured_xdr<R>(r: &mut R) -> ReadResult<Self>
        where R: Reader<'r> + AsRef<Path>
    {
        let path = r.as_ref().clone();

        r.seq(Some(2))?;

        let left = read_structured_xdr(r)?;
        r.next()?;

        let right = read_structured_xdr(r)?;
        r.next()?;  // extra next() calls are allowed before end()

        r.end()?;

        Ok(Node { path, left, right })
    }
}

#[test]
fn nodes() {
    // Create a tree of Nodes, along with the expected path values
    let path = Path::new();
    let root = mktree(&path).unwrap();
    let root = root.as_ref();

    // Encode the tree, dropping all the path values
    let buf = CXS::encode(root).unwrap();

    // Decode the tree, constructing path values
    let root2: Node = CXS::decode_structured(&buf, Path::new()).unwrap();

    // Assert that the decoded tree is the same as the expected one
    assert_eq!(root, &root2);

    let mut output = String::new();
    root.write(&mut output).unwrap();
    println!("TREE:\n{}", output);
}
