// What generated crates rely on from x23c's Rust output.  check.sh
// generates Rust from this and type-checks it with the uses in src/lib.rs.

const MAX_BATCH = 4;
const SIG_SIZE = 64;

enum Status {
  OK = 0,
  REJECTED = 1
};

// An enum value may name a member of the same enum or of another, or a
// constant.
enum Aliased {
  FIRST = 7,
  SAME = FIRST,
  OTHER = REJECTED,
  CONSTANT = MAX_BATCH
};

// #[rust(newtype)]
typedef opaque Hash[32];

struct Tx {
  unsigned hyper seq;
  opaque memo<16>;
  string note<32>;
  Hash hash;
  opaque digest[32];
  opaque sig[SIG_SIZE];
};

typedef Tx TxBatch<MAX_BATCH>;
typedef unsigned hyper Amount;

// A schema type with the name a generic parameter would most likely take.
typedef int T;

union Outcome switch (Status status) {
case OK:
  Hash hash;
case REJECTED:
  void;
};

struct Receipt {
  Outcome outcome;
  Tx *tx;
};

// Floats lack Eq, Hash and Ord.  The marker must reach the inline types too.
// #[rust(no_eq)]
struct Reading {
  float level;
  struct {
    double total;
  } sum;
  union switch (bool ok) {
  case TRUE:
    double value;
  case FALSE:
    void;
  } last;
};

// Case values have the switch type: unsigned cases of 2^31 and up, and
// negative signed ones, must compile and keep their value.
// A schema constant may have any name, including one x23c might pick for
// a temporary.
const V = 9;

union Unsigned switch (unsigned int k) {
case 4294967295:
  int v;
case 0x80000000:
  void;
case MAX_BATCH:
  void;
case V:
  void;
};

union Signed switch (int k) {
case -1:
  int v;
case -2147483648:
  void;
};

// A typedef'd switch type, which x23c cannot resolve: rustc does, through
// case values written as constants of the switch type.
typedef unsigned int Index;
typedef bool Flag;

union ByIndex switch (Index i) {
case 4294967295:
  int v;
case 1:
  void;
};

union ByFlag switch (Flag f) {
case TRUE:
  int v;
case FALSE:
  void;
};

// Lowercase `true` and `false` are accepted, with a warning, as `TRUE` and
// `FALSE`, whatever the switch type.
union LowerBool switch (bool b) {
case true:
  int v;
case false:
  void;
};

union LowerInt switch (int k) {
case true:
  int v;
case 2:
  void;
};

// The accessor's default arm, and several cases sharing an arm.
union UnsignedDefault switch (unsigned int k) {
case 4294967295:
case 0x80000000:
  int v;
case 7:
  void;
default:
  void;
};

union SignedDefault switch (int k) {
case -1:
case MAX_BATCH:
  void;
default:
  int other;
};

union BoolDefault switch (bool ok) {
case TRUE:
  int v;
default:
  void;
};

// A typedef of an enum keeps enum member labels; a single-case union over
// a typedef'd int has a single accessor arm.
typedef Status Verdict;
typedef int Code;

union ByVerdict switch (Verdict v) {
case OK:
  int n;
case REJECTED:
  void;
};

union ByCode switch (Code c) {
case 0:
  void;
};

// The const writers splice the typed case values too.
// #[rust(ConstWriteXDR)]
union ConstUnsigned switch (Index i) {
case 4294967295:
  int v;
case 0xA:
  void;
};

// A default arm switched on a typedef'd primitive. Its field has the
// primitive's type, which the const writers recognize; they would not
// recognize the alias.
// #[rust(ConstWriteXDR)]
union ConstAliasDefault switch (Index i) {
case 1:
  int v;
default:
  void;
};

// An anonymous union inside a struct.
struct Tagged {
  union switch (unsigned int k) {
  case 4294967295:
    int v;
  case 0:
    void;
  } tag;
};

// Left implicit, the default variant's tag would be the last case's plus
// one, which here is another case's value: DOWN + 1 = IDLE, and
// 4294967295 (-1 as an i32) + 1 = 0.  x23c has rustc pick a free tag.
enum Signal {
  DOWN = -1,
  IDLE = 0,
  UP = 1
};

union BySignal switch (Signal s) {
case IDLE:
  int n;
case DOWN:
  void;
default:
  void;
};

// The tag's helper constant must not shadow a schema constant either.
const CASES = 11;

union Wrapping switch (unsigned int k) {
case CASES:
  void;
case 0:
  void;
case 4294967295:
  int v;
default:
  int other;
};

program check {
  version check_v1 {
    void ping(void) = 0;
    int add(int, unsigned hyper) = 1;
    Outcome submit(TxBatch, bool) = 2;
    Receipt credit(Amount, Tx) = 3;
    bool verify(Hash) = 4;
    TxBatch pending(void) = 5;
    T scale(T) = 6;
    double ratio(float, Reading) = 7;
  } = 1;
} = 0x20000003;
