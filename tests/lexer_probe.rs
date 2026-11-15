// SPDX-License-Identifier: GPL-3.0-or-later
// Test-only adapter. This is not an rpp CLI operation.
#[allow(dead_code)]
#[path = "../src/lex/mod.rs"]
mod lex;
use std::io::Write;
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let edition = match args[1].as_str() {
        "2021" => lex::Edition::Rust2021,
        "2024" => lex::Edition::Rust2024,
        _ => panic!("test adapter requires 2021 or 2024"),
    };
    let bytes = std::fs::read(&args[2]).unwrap();
    match lex::scan_bytes(&bytes, edition) {
        Ok(source) => {
            let mut out = std::io::stdout().lock();
            for token in &source.tokens {
                out.write_all(&bytes[token.span.start..token.span.end])
                    .unwrap();
            }
        }
        Err(e) => {
            eprintln!("{:?}\t{}\t{}", e.kind, e.span.start, e.span.end);
            std::process::exit(1);
        }
    }
}
