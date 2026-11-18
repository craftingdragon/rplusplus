// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;

#[test]
fn fragments_do_not_recognize_directives_or_file_prologues() {
    let l = scan_fragment("#error text", Edition::Rust2024).unwrap();
    assert!(
        !l.tokens
            .iter()
            .any(|t| matches!(t.kind, Kind::DirectiveLine { .. } | Kind::Shebang))
    );
    assert!(scan_fragment("\u{feff}x", Edition::Rust2024).is_err());
    assert_eq!(
        source_position("\"unfinished\r\n🦀", 17),
        Some(Position {
            byte: 17,
            line: 2,
            column: 2
        })
    );
}

fn lex(s: &str) -> Lexed<'_> {
    scan(s, Edition::Rust2024).unwrap()
}
fn parts(s: &str) -> Vec<(Kind, &str)> {
    let l = lex(s);
    l.tokens
        .iter()
        .map(|t| (t.kind, &s[t.span.start..t.span.end]))
        .collect()
}
fn roundtrip(s: &str, edition: Edition) {
    let a = scan(s, edition).unwrap();
    let b = scan(s, edition).unwrap();
    assert_eq!(a.tokens, b.tokens);
    assert_eq!(a.protected, b.protected);
    let mut at = 0;
    for t in &a.tokens {
        assert_eq!(t.span.start, at);
        assert!(t.span.end > at);
        assert!(s.is_char_boundary(t.span.start) && s.is_char_boundary(t.span.end));
        at = t.span.end;
    }
    assert_eq!(at, s.len());
    assert_eq!(
        a.tokens.iter().map(|t| a.text(t.span)).collect::<String>(),
        s
    );
}

#[test]
fn unicode_identifiers_are_whole_tokens() {
    for name in [
        "café",
        "cafe\u{301}",
        "東京",
        "Москва",
        "NAME\u{301}",
        "\u{2118}x",
        "\u{10400}x",
    ] {
        assert_eq!(parts(name), vec![(Kind::Identifier { raw: false }, name)]);
        roundtrip(name, Edition::Rust2024);
    }
    assert!(scan("\u{301}NAME", Edition::Rust2024).is_err());
    assert!(scan("NAME\u{200d}other", Edition::Rust2024).is_err());
    // Neither NFC normalization nor keyword validation belongs to this scanner.
    assert_eq!(
        lex("gen async self")
            .tokens
            .iter()
            .filter(|t| matches!(t.kind, Kind::Identifier { .. }))
            .count(),
        3
    );
}

#[test]
fn unicode_tables_are_sorted_and_start_is_subset() {
    for table in [unicode::XID_START, unicode::XID_CONTINUE] {
        for (i, &(a, b)) in table.iter().enumerate() {
            assert!(a <= b && b <= 0x10ffff);
            if i > 0 {
                assert!(table[i - 1].1 < a);
            }
        }
    }
    for &(a, b) in unicode::XID_START {
        for n in a..=b {
            let c = char::from_u32(n).unwrap();
            let i = unicode::XID_CONTINUE.partition_point(|&(lo, _)| lo <= n);
            assert!(i > 0 && n <= unicode::XID_CONTINUE[i - 1].1);
            assert_eq!(
                lex(&c.to_string()).tokens[0].kind,
                Kind::Identifier { raw: false }
            );
        }
    }
}

#[test]
fn literals_include_suffixes() {
    let cases = [
        ("'x'", Literal::Character),
        ("'🦀'tag", Literal::Character),
        ("b'\\xff'", Literal::Byte),
        ("\"quoted\\\"text\"suffix", Literal::String),
        ("b\"bytes\\xFF\"NAME", Literal::ByteString),
        ("c\"Grüezi\"", Literal::CString),
        ("r###\"NAME \"## still inside\"###tail", Literal::RawString),
        ("br#\"NAME\"#suffix", Literal::RawByteString),
        ("cr\"C text\"", Literal::RawCString),
        ("123u64", Literal::Number),
        ("0xffNAME", Literal::Number),
        ("1.0e-2f64", Literal::Number),
        ("1_2.3_4E+5_6", Literal::Number),
        ("0b1_0suffix", Literal::Number),
        ("0o77suffix", Literal::Number),
    ];
    for (s, kind) in cases {
        assert_eq!(parts(s), vec![(Kind::Literal(kind), s)], "{s}");
    }
}

#[test]
fn raw_string_hash_boundaries() {
    for n in [0, 1, 2, 254, 255] {
        let h = "#".repeat(n);
        roundtrip(&format!("r{h}\"text\"{h}"), Edition::Rust2024);
    }
    let h = "#".repeat(256);
    assert_eq!(
        scan(&format!("r{h}\"text\"{h}"), Edition::Rust2024)
            .unwrap_err()
            .kind,
        ErrorKind::InvalidRawDelimiter
    );
}

#[test]
fn numbers_do_not_eat_paths_ranges_or_tuple_fields() {
    let s = "1.foo() 1..2 1..=2 1. 1.0 0.1.0 1._x";
    let p = parts(s);
    assert_eq!(p[0], (Kind::Literal(Literal::Number), "1"));
    assert_eq!(p[1], (Kind::Punct('.'), "."));
    assert_eq!(p[2].1, "foo");
    assert!(
        p.iter()
            .any(|&(k, t)| k == Kind::Literal(Literal::Number) && t == "1.")
    );
    roundtrip(s, Edition::Rust2024);
}

#[test]
fn escapes_and_crlf_preserve_raw_bytes() {
    for s in [
        "'\\u{1_f980}'",
        "'\\''",
        "\"a\\\r\n  b\"",
        "r\"a\r\nb\"",
        "\"a\r\nb\"",
        "c\"\\xFF\"",
        "b\"\\0\\xff\"",
    ] {
        roundtrip(s, Edition::Rust2024);
    }
}

#[test]
fn lifetime_raw_identifier_and_label_boundaries() {
    let p = parts("'a 'static '_ 'r#gen 'label: r#type r#gen");
    assert_eq!(p[0], (Kind::Lifetime { raw: false }, "'a"));
    assert!(p.contains(&(Kind::Lifetime { raw: true }, "'r#gen")));
    assert!(p.contains(&(Kind::Lifetime { raw: false }, "'label")));
    assert!(p.contains(&(Kind::Identifier { raw: true }, "r#type")));
    roundtrip(
        "fn f<'a: 'b, 'b>() { 'label: loop {break 'label;} }",
        Edition::Rust2021,
    );
}

#[test]
fn nested_comments_and_doc_comments_remain_trivia() {
    let s = "/// NAME\r\n//! NAME\n/* a /* nested */ b */ /** doc */ /*! inner */";
    let l = lex(s);
    assert!(l.tokens.iter().all(|t| t.kind.trivia()));
    roundtrip(s, Edition::Rust2024);
}

#[test]
fn attributes_and_macro_paths_are_protected() {
    let s = "#[derive(NAME)] #![allow(NAME)] ::path::NAME! { NAME; nested!(NAME); #[NAME] } macro_rules! NAME { ($NAME:ident) => { $NAME }; } NAME";
    let l = lex(s);
    let eligible: Vec<_> = l
        .tokens
        .iter()
        .filter(|t| l.substitutable(t))
        .map(|t| l.text(t.span))
        .collect();
    assert_eq!(eligible, ["NAME"]);
    assert!(
        l.protected
            .iter()
            .any(|r| r.kind == Protection::MacroDefinition)
    );
    assert!(l.protected.iter().any(
        |r| r.kind == Protection::MacroInvocation && l.text(r.span).starts_with("::path::NAME")
    ));
    roundtrip(s, Edition::Rust2024);
}

#[test]
fn macro_all_delimiters_trivia_and_raw_names() {
    for s in [
        "r#NAME /*c*/ ! /*c*/ ( NAME )",
        "NAME![ NAME ]",
        "NAME!{ NAME }",
        "macro_rules /*c*/ ! r#NAME ( ($x:tt)=>{$x}; );",
        "#[proc_macro] pub fn f(){}",
    ] {
        roundtrip(s, Edition::Rust2021);
        let l = lex(s);
        assert!(
            l.tokens
                .iter()
                .filter(|t| l.text(t.span) == "NAME")
                .all(|t| !l.substitutable(t))
        );
        assert!(!l.protected.is_empty());
    }
}

#[test]
fn operators_are_not_mistaken_for_macro_bangs() {
    let l = lex("if !NAME { NAME; } if NAME != (NAME) {} if NAME!() {}");
    assert_eq!(l.protected.len(), 1);
    assert_eq!(l.text(l.protected[0].span), "NAME!()");
}

#[test]
fn directives_are_physical_line_candidates_only() {
    let s = "  #define NAME anything can't be Rust here\r\n# if NAME\nfn f(){\n\t#undef NAME\n}\n#endif\n#unknown text\n";
    let l = lex(s);
    let names: Vec<_> = l
        .tokens
        .iter()
        .filter_map(|t| {
            if let Kind::DirectiveLine { name } = t.kind {
                Some(l.text(name))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(names, ["define", "if", "undef", "endif", "unknown"]);
    assert!(
        l.tokens
            .iter()
            .any(|t| t.kind == Kind::Whitespace && l.text(t.span) == "\r\n")
    );
    roundtrip(s, Edition::Rust2024);
}

#[test]
fn directive_lookalikes_stay_inside_non_code() {
    let s = "r#\"\n#define NAME\n\"#; /*\n#define NAME\n*/\nNAME!{\n#define NAME 1\n}\n#[NAME]\n\u{200e}#define NAME\n/* c */ #define NAME\n";
    let l = lex(s);
    assert!(
        !l.tokens
            .iter()
            .any(|t| matches!(t.kind, Kind::DirectiveLine { .. }))
    );
    roundtrip(s, Edition::Rust2024);
}

#[test]
fn directive_contents_are_not_lexed_or_executed() {
    let l = lex("#error \"unterminated arbitrary message\n#nonsense (((\n#define X \"abc\"\n");
    assert_eq!(
        l.tokens
            .iter()
            .filter(|t| matches!(t.kind, Kind::DirectiveLine { .. }))
            .count(),
        3
    );
}

#[test]
fn bom_shebang_and_inner_attributes() {
    for s in [
        "\u{feff}#!/usr/bin/rpp\r\nfn main(){}",
        "#! /*comment*/ [allow(dead_code)]\nfn main(){}",
        "#! // comment\n[allow(dead_code)]",
        "#!/*unterminated shebang text\nfn main(){}",
    ] {
        roundtrip(s, Edition::Rust2024);
    }
    let l = lex("\u{feff}#define X 1\n");
    assert_eq!(l.tokens[0].kind, Kind::Bom);
    assert!(matches!(l.tokens[1].kind, Kind::DirectiveLine { .. }));
    assert_eq!(
        lex("#![allow(dead_code)]").protected[0].kind,
        Protection::Attribute
    );
    assert_eq!(lex("#!/bin/x").tokens[0].kind, Kind::Shebang);
}

#[test]
fn positions_use_raw_bytes_and_scalar_columns() {
    let s = "\u{feff}aé\t\r\n🦀";
    // Emoji alone is not a Rust token; put it in a string for tokenization.
    let s = format!("{}\"🦀\"", &s[..s.find('🦀').unwrap()]);
    let l = lex(&s);
    for (byte, line, column) in [
        (0, 1, 1),
        (3, 1, 1),
        (4, 1, 2),
        (6, 1, 3),
        (7, 1, 4),
        (8, 1, 4),
        (9, 2, 1),
        (10, 2, 2),
        (14, 2, 3),
        (15, 2, 4),
    ] {
        assert_eq!(l.position(byte), Some(Position { byte, line, column }));
    }
    assert_eq!(l.position(5), None);
    assert_eq!(l.position(s.len() + 1), None);
}

#[test]
fn edition_2024_guarded_syntax_is_distinct() {
    for s in ["m!(###)", "m!(#\"text\"#)"] {
        roundtrip(s, Edition::Rust2021);
        assert_eq!(
            scan(s, Edition::Rust2024).unwrap_err().kind,
            ErrorKind::ReservedSyntax
        );
    }
    roundtrip("m!(# # #)", Edition::Rust2024);
}

#[test]
fn malformed_inputs_have_stable_explicit_errors() {
    let cases = [
        ("/* nested /* x */", ErrorKind::UnterminatedComment),
        ("\"unterminated", ErrorKind::UnterminatedLiteral),
        ("r#\"unfinished", ErrorKind::UnterminatedLiteral),
        ("'ab'", ErrorKind::InvalidLiteral),
        ("''", ErrorKind::InvalidLiteral),
        ("'\\q'", ErrorKind::InvalidEscape),
        ("\"\\xGG\"", ErrorKind::InvalidEscape),
        ("\"\\u{d800}\"", ErrorKind::InvalidEscape),
        ("\"\\u{110000}\"", ErrorKind::InvalidEscape),
        ("\"\\u{_1}\"", ErrorKind::InvalidEscape),
        ("b'🦀'", ErrorKind::InvalidLiteral),
        ("b\"é\"", ErrorKind::InvalidLiteral),
        ("br\"é\"", ErrorKind::InvalidLiteral),
        ("c\"\\0\"", ErrorKind::InvalidEscape),
        ("cr\"\0\"", ErrorKind::InvalidLiteral),
        ("\"bare\rCR\"", ErrorKind::InvalidLiteral),
        ("r#self", ErrorKind::ReservedSyntax),
        ("'r#_", ErrorKind::ReservedSyntax),
        ("foo\"x\"", ErrorKind::ReservedSyntax),
        ("r##x", ErrorKind::ReservedSyntax),
        ("0x__", ErrorKind::InvalidLiteral),
        ("0b2", ErrorKind::InvalidLiteral),
        ("0b1e2", ErrorKind::InvalidLiteral),
        ("0xff.0", ErrorKind::InvalidLiteral),
        ("2e+", ErrorKind::InvalidLiteral),
        ("🦀", ErrorKind::UnknownCharacter),
        ("#[NAME", ErrorKind::UnclosedDelimiter),
        ("NAME!([)]", ErrorKind::MismatchedDelimiter),
        ("NAME!{", ErrorKind::UnclosedDelimiter),
    ];
    for (s, expected) in cases {
        let a = scan(s, Edition::Rust2024).unwrap_err();
        assert_eq!(a.kind, expected, "{s:?}");
        assert_eq!(a, scan(s, Edition::Rust2024).unwrap_err());
        assert!(a.span.start <= a.span.end && a.span.end <= s.len());
    }
    assert_eq!(
        scan_bytes(b"abc\xff", Edition::Rust2024).unwrap_err().kind,
        ErrorKind::InvalidUtf8
    );
}

#[test]
fn ordinary_rust_grammar_errors_are_left_to_rustc() {
    roundtrip("fn broken( { let = ; )", Edition::Rust2024);
    roundtrip("'a: 'b", Edition::Rust2024);
}

#[test]
fn deeply_nested_trees_use_bounded_iterative_state() {
    let s = format!("m!({}NAME{})", "(".repeat(4096), ")".repeat(4096));
    assert_eq!(
        scan(&s, Edition::Rust2024).unwrap_err().kind,
        ErrorKind::NestingLimit
    );
    roundtrip(
        &format!("m!({}NAME{})", "(".repeat(1024), ")".repeat(1024)),
        Edition::Rust2024,
    );
}

#[test]
fn deterministic_prefix_and_random_input_regression() {
    let corpus =
        "#[a] macro_rules! m {($a:tt)=>{r#\"NAME\"#};} fn f<'a>() { m!('a); }\r\n#define X 1\n";
    for (i, _) in corpus
        .char_indices()
        .chain(std::iter::once((corpus.len(), '\0')))
    {
        if scan(&corpus[..i], Edition::Rust2024).is_ok() {
            roundtrip(&corpus[..i], Edition::Rust2024);
        }
    }
    let alphabet: Vec<char> = "a_0é\u{301}#'\"\\/*()[]{}!\r\n🦀".chars().collect();
    let mut state = 7u64;
    for _ in 0..2000 {
        let mut s = String::new();
        for _ in 0..48 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            s.push(alphabet[(state >> 32) as usize % alphabet.len()]);
        }
        if scan(&s, Edition::Rust2024).is_ok() {
            roundtrip(&s, Edition::Rust2024);
        }
    }
}
