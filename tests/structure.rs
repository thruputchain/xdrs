mod common;
use common::*;

use xdrs::*;
use xdrs::serialize::reader::*;

use std::vec::Vec;
use std::vec;

#[derive(Clone, Debug, PartialEq, WriteXDR)]
#[xdrs(as_type = "()")]
struct Path(Vec<usize>);

impl<'r> ReadXDR<'r, Path> for Path {
    fn read_xdr<R>(r: &mut R) -> ReadResult<Self>
        where R: Reader<'r, Path>
    {
        Ok(r.as_ref().clone())
    }
}

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

extern crate alloc;
use alloc::boxed::Box;

#[derive(Debug, PartialEq, WriteXDR)]
struct Node {
    path: Path,
    left: Option<Box<Node>>,
    right: Option<Box<Node>>,
    okay: bool,
    paths: [Path; 1],
    // bytes: [u8; 3],
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
            okay: false,
            paths: [path.child(3).child(0)],
            // bytes: [7; 3],
        }))
    }
}

// Manually implement reading in the same way that will be derived
impl<'r> ReadXDR<'r, Path> for Node {
    fn read_xdr<R>(r: &mut R) -> ReadResult<Self>
        where R: Reader<'r, Path>
    {
        let path = read_xdr(r)?;

        r.seq(Some(2))?;

        let left = read_xdr(r)?;
        r.next()?;

        let right = read_xdr(r)?;
        r.next()?;

        let okay = read_xdr(r)?;
        r.next()?;

        let paths = read_xdr(r)?;
        r.next()?;

        /*
        let bytes = read_xdr(r)?;
        r.next()?; // extra next() calls are allowed before end()
        */

        r.end()?;

        Ok(Node { path, left, right, okay, paths, /* bytes */ })
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
