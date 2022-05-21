#![feature(proc_macro_span)]

use json::JsonValue;
use lazy_static::lazy_static;
use proc_macro::{Span, TokenStream};
use std::collections::{HashMap, HashSet};
use std::process;
use std::str::FromStr;
use std::sync::Mutex;

#[proc_macro]
pub fn magic(input: TokenStream) -> TokenStream {
    assert!(
        input.is_empty(),
        "auto_import::magic!() takes no arguments!"
    );

    let file = Span::call_site().source_file();
    if !file.is_real() {
        // I don't know why this would ever be false or what a fake file even means, so don't handle it
        return input;
    }

    // JSON output contains paths which ig is UTF-8 too. not quite sure what that's about.
    // i think this'll panic with non-UTF8 stuff because of that, so therefore i assume valid UTF-8
    let file = file.path();
    let file = file.to_str().expect("valid UTF-8");

    // uhh idk what's valid in env vars, from a quick google search it seems just alphanumeric and _ so better safe than sorry
    let custom_key: String = "autoimport_"
        .chars()
        .chain(file.chars().filter(char::is_ascii_alphanumeric))
        .collect();
    let custom_key = custom_key.as_str();

    if let Ok(x) = std::env::var(custom_key) {
        return TokenStream::from_str(&x).unwrap();
    }

    // autoimport launched this process to check for errors, but this is NOT the correct invocation of the macro
    if let Ok(_) = std::env::var("autoimport") {
        return input;
    }

    lazy_static! {
        static ref ONCE: Mutex<HashSet<String>> = Mutex::new(HashSet::new());
    }

    {
        let mut files = ONCE.lock().unwrap();
        if files.contains(custom_key) {
            // this poisons future invocations but uh, i guess that just prevents extra resources from being used for invalid invocations
            panic!("don't call auto_import::magic!() more than once per file!");
        }
        files.insert(custom_key.to_string());
    }

    let mut imports = HashSet::<String>::new();
    let mut more_imports = HashSet::<String>::new();
    let mut excluded = HashSet::<String>::new();

    let mut attempts = 0;
    loop {
        attempts += 1;
        let mut change = false;
        let mut args = std::env::args_os();
        let out = process::Command::new(args.next().unwrap())
            .args(args.filter(|arg| {
                arg.to_str()
                    .map_or(true, |s| !s.starts_with("--error-format="))
            }))
            .arg("--error-format=json")
            .env("autoimport", "YES_SO_DONT_EVEN_TRY_ANYTHING")
            .env(
                custom_key,
                imports
                    .iter()
                    .flat_map(|s| ["use ", s, ";"])
                    .collect::<String>(),
            )
            .output()
            .unwrap();
        if out.status.success() {
            process::exit(0);
        }
        for line in std::str::from_utf8(&out.stderr)
            .unwrap()
            .lines()
            .filter(|l| l.starts_with('{'))
        {
            if let Ok(json) = json::parse(line) {
                // Ensure the file name matches.
                if json["spans"].members().any(|span| {
                    // assert_eq will contain "similarly named macro `assert` defined here"
                    // with "is_primary": false, so therefore only check path for the
                    span["is_primary"].as_bool().unwrap_or(false)
                        && span["file_name"]
                            .as_str()
                            .map_or(false, |error_file| error_file != file)
                }) {
                    continue;
                }
                if json["children"].members().any(|c| {
                    c["spans"].members().any(|span| {
                        span["file_name"]
                            .as_str()
                            .map_or(false, |error_file| error_file != file)
                    })
                }) {
                    continue;
                }
                let suggestions: Vec<&str> = error(&json)
                    .into_iter()
                    .filter(|&s| !imports.contains(s))
                    .collect();
                if !suggestions.is_empty() {
                    more_imports.extend(
                        suggestions
                            .into_iter()
                            .filter(|&s| !excluded.contains(s))
                            .map(Into::into),
                    );
                }
            }
        }

        for import in &imports {
            more_imports.remove(import);
        }

        if more_imports.len() > 1 {
            let mut idents: HashMap<String, Vec<String>> = HashMap::new();
            for suggestion in more_imports.drain() {
                let ident = suggestion.split("::").last().unwrap();
                let suggestions_for_ident = idents.entry(ident.to_string()).or_default();
                suggestions_for_ident.push(suggestion);
            }
            for (ident, suggestions) in idents {
                let (best, exclude) = disambiguate(ident, suggestions);
                println!("\x1b[1;32m   Injecting\x1b[m for {best}");
                imports.insert(best);
                for bad in exclude {
                    excluded.insert(bad);
                }
            }
            change = true;
        } else if more_imports.len() == 1 {
            imports.extend(more_imports.drain());
            change = true;
        }

        if !change || attempts == 10 {
            return TokenStream::from_str(
                &imports
                    .iter()
                    .flat_map(|s| ["use ", s, ";"])
                    .collect::<String>(),
            )
            .unwrap();
        }
    }
}

fn error<'a>(json: &'a JsonValue) -> Vec<&'a str> {
    if json["code"].is_null() {
        let message = json["message"].as_str().unwrap_or_default();
        if extract("cannot find macro `", message, "` in this scope").is_some() {
            let message = json["children"][0]["message"].as_str().unwrap_or_default();
            if let Some(suggestions) =
                extract("consider importing one of these items:", message, "")
            {
                return suggestions
                    .split_terminator("\n")
                    .filter(|s| !s.is_empty())
                    .collect();
            } else if let Some(suggestion) =
                extract("consider importing this macro:\n", message, "")
            {
                return vec![suggestion];
            }
        }
    }
    json["children"]
        .members()
        .flat_map(|c| {
            c["spans"]
                .members()
                .map(|s| s["suggested_replacement"].as_str().unwrap_or_default())
                .filter(|s| !s.is_empty())
                .filter_map(|s| extract("use ", s.trim(), ";"))
        })
        .collect()
}

fn extract<'a>(start: &'static str, message: &'a str, end: &'static str) -> Option<&'a str> {
    if message.starts_with(start) && message.ends_with(end) {
        Some(&message[start.len()..(message.len() - end.len())])
    } else {
        None
    }
}

fn disambiguate(ident: String, mut suggestions: Vec<String>) -> (String, Vec<String>) {
    assert!(!suggestions.is_empty());
    if suggestions.len() == 1 {
        return (suggestions.remove(0), Vec::new());
    }
    for i in 0..(suggestions.len() - 1) {
        for j in (i + 1)..suggestions.len() {
            if std_and_core(&ident, &suggestions[i], &suggestions[j]) {
                suggestions.swap_remove(j);
                return disambiguate(ident, suggestions);
            } else if std_and_core(&ident, &suggestions[j], &suggestions[i]) {
                suggestions.swap_remove(i);
                return disambiguate(ident, suggestions);
            }
        }
    }

    println!("\x1b[1;32m   Ambiguity\x1b[m for {ident}");
    println!("\x1b[1;32m     Between\x1b[m {} items", suggestions.len());
    for import in &suggestions {
        println!("\x1b[1;32m            \x1b[m {import}");
    }
    const DEFAULTS: &[&str] = &[
        "std::ops::Range", // also includes BTreeMap/BTreeSet ranges
    ];

    if let Some(index) = suggestions
        .iter()
        .position(|s| DEFAULTS.contains(&s.as_str()))
    {
        let result = suggestions.swap_remove(index);
        println!("\x1b[1;32m     Picking\x1b[m {result}");
        return (result, suggestions);
    }

    use rand::prelude::*;

    println!("\x1b[1;32m  Don't know\x1b[m which is best");
    println!("\x1b[1;32m     Picking\x1b[m at random");
    let index = (0..suggestions.len()).choose(&mut thread_rng()).unwrap();
    let result = suggestions.swap_remove(index);
    println!("\x1b[1;32mEnded up with\x1b[m {result}");
    return (result, suggestions);
}

#[allow(non_upper_case_globals)]
fn std_and_core(ident: &str, a: &str, b: &str) -> bool {
    const std: &str = "std::";
    const core: &str = "core::";
    let r = a.starts_with(std) && b.starts_with(core) && a[std.len()..] == b[core.len()..];
    if r {
        println!("\x1b[1;32m   Ambiguity\x1b[m for {ident}");
        println!("\x1b[1;32m     Between\x1b[m 2 items");
        println!("\x1b[1;32m            \x1b[m {a}");
        println!("\x1b[1;32m            \x1b[m {b}");
        println!("\x1b[1;32m     Picking\x1b[m {a}");
    }
    r
}
