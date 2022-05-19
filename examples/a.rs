auto_import::magic!();

fn main() {
    let _ = BTreeMap::<File, PathBuf>::new();
    let _ = i32::from_str("123");
    std::io::stdout().write_all(b"!\n").unwrap();
}
