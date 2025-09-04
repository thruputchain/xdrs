mod common;
use common::*;

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::{vec, vec::Vec};

use xdrs::serialize::reader::*;
use xdrs::serialize_cxs::buf_reader::*;

struct Path {
    depth: usize,
    index: usize,
}

impl Path {
    fn new() -> Self {
        Self { depth: 0, index: 0 }
    }
}

impl Structured<Path> for Path {
    fn seq(&mut self, len: Option<usize>) -> Result<()> {
        self.depth += 1;
        self.index = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<()> {
        self.index += 1;
        Ok(())
    }

    fn end(&mut self) -> Result<()> {
        self.depth = self.depth.checked_sub(1).unwrap(); // XXX
        Ok(())
    }

    fn structure(&self) -> &Path {
        self
    }
}

#[test]
fn array() {
}
