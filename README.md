# auto-import

The `auto_import::magic!{}` macro expands to whatever
`use` statements you need to make the rest of the code compile.

https://twitter.com/m_ou_se/status/1527209443309633536

Please do not use this.

## Example

```rust
auto_import::magic!();

fn main() {
    let _ = BTreeMap::<File, PathBuf>::new();
    let _ = i32::from_str("123");
    std::io::stdout().write_all(b"!\n").unwrap();
}
```

```
$ cargo run
   Compiling auto-import v0.1.0
   Compiling example v0.1.0
   Injecting use std::collections::BTreeMap;
   Injecting use std::fs::File;
   Injecting use std::path::PathBuf;
   Injecting use std::str::FromStr;
   Injecting use std::io::Write;
    Finished dev [unoptimized + debuginfo] target(s) in 0.60s
     Running `target/debug/example`
!
```
