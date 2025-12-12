# status
#   - the ToTokens implementation seems pretty complete 
#   - we should test the round-trip of example XDR files

cargo test --color=always -F repr,alloc,derive --test repr macros  -- --show-output
