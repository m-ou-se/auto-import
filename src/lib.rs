use proc_macro::TokenStream;
use std::collections::BTreeSet;
use std::io::{stderr, stdout, Write};
use std::process::{exit, Command};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

#[proc_macro]
pub fn magic(input: TokenStream) -> TokenStream {
    assert!(
        input.is_empty(),
        "auto_import::magic!() takes no arguments!"
    );

    static ONCE: AtomicBool = AtomicBool::new(false);

    if ONCE.swap(true, Relaxed) {
        panic!("don't call auto_import::magic!() more than once per crate!");
    }

    if let Ok(x) = std::env::var("autoimport") {
        return TokenStream::from_str(&x).unwrap();
    }

    let mut imports = BTreeSet::new();

    let mut attempts = 0;
    loop {
        attempts += 1;
        let mut change = false;
        let mut args = std::env::args_os();
        let out = Command::new(args.next().unwrap())
            .args(args.filter(|arg| {
                arg.to_str()
                    .map_or(true, |s| !s.starts_with("--error-format="))
            }))
            .arg("--error-format=json")
            .env(
                "autoimport",
                imports.iter().map(String::as_str).collect::<String>(),
            )
            .output()
            .unwrap();
        if out.status.success() {
            exit(0);
        }
        for line in std::str::from_utf8(&out.stderr)
            .unwrap()
            .lines()
            .filter(|l| l.starts_with('{'))
        {
            if let Ok(d) = json::parse(line) {
                for c in d["children"].members() {
                    let suggestion = c["spans"][0]["suggested_replacement"]
                        .as_str()
                        .unwrap_or_default();
                    if c["spans"][0]["text"].is_empty()
                        && suggestion.starts_with("use ")
                        && imports.insert(suggestion.to_string())
                    {
                        println!("\x1b[1;32m   Injecting\x1b[m {}", suggestion.trim());
                        change = true;
                    }
                }
            }
        }
        if !change || attempts == 10 {
            stderr().write_all(&out.stderr).unwrap();
            stdout().write_all(&out.stdout).unwrap();
            exit(out.status.code().unwrap_or(1));
        }
    }
}
