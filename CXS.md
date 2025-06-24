# CXS (Compact XDR Serialization)

This modification of the traditional XDR serialization (henceforth *TXS*) makes
these changes:
- No 32-bit block structure: structures and arrays are not padded
- Variable-length encoding for integer types and sizes of variable-length arrays

The changes from TXS in detail:

- XDR _Bool_, _Integer_, _Unsigned Integer_, _Hyper_, _Unsigned Hyper_ are
  encoded as bytes `H, ...`, where header-byte `H` has bit-fields `s` (2 bits)
  followed by `v` (6 bits), which are used in combination with 0 to 8 following
  bytes to specify 1 to 8 "integer bytes" as follows (where the decimal values
  for `s` and `v` below are the unsigned interpretations of their bits):
    - `s = 0, v = 0..62`:
        - For unsigned types, the single integer byte is `H`
        - For signed types, `v` represents the value with 6-bit,
          one's-complement encoding.  The single integer byte is the 8-bit,
          two's-complement encoding of that value.
    - `s = 0, v = 63`: the single integer byte is the following byte
    - `s = 1, v = 63`: the integer bytes are the following 2 bytes
    - `s = 2, v = 63`: the integer bytes are the following 4 bytes
    - `s = 3, v = 63`: the integer bytes are the following 8 bytes
    - all other `s, v` values are reserved

  The resulting XDR integer value is the little-endian interpretation of the
  integer bytes, using two's complement for the signed XDR types.  Encodings
  other than the shortest possible are invalid.

- XDR _Structure_ :
   same as TXS, but with no padding

- XDR _Fixed-Length Array_ / _Fixed-Length Opaque Data_ :
   same as TXS, but with no padding

- XDR _Variable-Length Array_ / _Variable-Length Opaque Data_ / _String_ :
   same as TXS, but with no padding and the length is indicated using the new
   encoding for XDR _Unsigned Integer_.

- XDR _Discriminated Union_:
   same as TXS, but the discriminant uses the new integer encoding

Serialization of these XDR types is also changed, according to their equivalent
representations:

- XDR _Enumeration_: equivalent to _Integer_

- XDR _Bool_: equivalent to `enum { FALSE = 0, TRUE = 1 }`

- XDR _Optional-Data_: equivalent to a _Discriminated Union_ with one void arm
  and one data arm, switched on a _Bool_. Also equivalent to a _Variable-Length
  Array_ with maximum length one.
