// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;

fn inputs(files: &[(&str, &str)]) -> Sources {
    let mut s = Sources::default();
    for (p, t) in files {
        s.insert(p, t.to_string()).unwrap();
    }
    s
}
fn run(text: &str) -> Result<Output, Error> {
    preprocess(
        &inputs(&[("root.rs", text)]),
        "root.rs",
        &Options::default(),
    )
}

#[test]
fn ordinary_rust_is_unchanged() {
    for text in [
        "",
        "\u{feff}#!/usr/bin/rpp\r\nfn main(){}\r\n",
        "#[allow(dead_code)] fn f<'a>(x:&'a str)->&'a str{x}",
        "macro_rules! m {($x:tt)=>{$x};} m!(NAME); // NAME\n",
    ] {
        let o = run(text).unwrap();
        assert_eq!(o.text, text);
        assert!(o.mappings.iter().all(|m| m.exact));
    }
}

#[test]
fn definitions_expansion_undef_and_redefinition() {
    let o =
        run("#define X 4\n#define Y X\nY;\n#undef X\nY;\n#define Y X\n#undef Y\n#define Y 9\nY;")
            .unwrap();
    assert!(o.text.contains("  4  ;"), "{}", o.text);
    assert!(o.text.contains(" X ;"));
    assert!(o.text.ends_with(" 9 ;"));
    assert_eq!(
        run("#define X 1\n#define X 2").unwrap_err().kind,
        ErrorKind::Definition
    );
    assert_eq!(
        run("#define F(x) x").unwrap_err().kind,
        ErrorKind::Definition
    );
    assert!(run("#define F (x)\nF;").is_ok());
    let o = run("#define X Y\nX;\n#define Y 3\nX;").unwrap();
    assert!(o.text.contains(" Y ;"));
    assert!(o.text.ends_with("  3  ;"));
}

#[test]
fn rust_non_code_and_macro_boundaries_stay_protected() {
    let o=run("#define NAME 7\nlet NAME=NAME; let s=r#\"NAME\"#; // NAME\n#[NAME] fn f<'NAME>() {m!(NAME);let r#NAME=0; 'NAME: loop{break 'NAME;}}\nmacro_rules! NAME {($NAME:ident)=>{$NAME};}").unwrap();
    assert!(o.text.contains("let  7 = 7 ;"));
    for p in [
        "r#\"NAME\"#",
        "// NAME",
        "#[NAME]",
        "'NAME",
        "m!(NAME)",
        "r#NAME",
        "macro_rules! NAME",
    ] {
        assert!(o.text.contains(p), "{p}: {}", o.text);
    }
    // Expansion creates a macro with the following original delimiter: protect
    // its argument before trying any subsequent identifier expansion.
    let o = run("#define CALL m!\n#define X 7\nCALL(X)").unwrap();
    assert!(o.text.contains("m! (X)"), "{}", o.text);
    let o = run("#define OPEN (\n#define X 7\nm! OPEN X)").unwrap();
    assert!(o.text.contains(" X)"));
}

#[test]
fn comments_do_not_swallow_following_code_and_tokens_do_not_paste() {
    let o =
        run("#define N 2 // explanation\n#define EMPTY\n#define STAR *\nN + 1; /STAR/; a EMPTY b;")
            .unwrap();
    assert!(o.text.contains("2  + 1;"));
    assert!(!o.text.contains("/*"));
    assert!(o.text.contains("a    b;"));
    assert!(run("#define P ##\nP").is_err());
    assert!(run("#define S #X").is_err());
}

#[test]
fn recursive_expansions_report_use_site_and_definitions() {
    for text in ["#define A A\nA", "#define A B\n#define B A\nA"] {
        let e = run(text).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Recursion);
        assert_eq!(e.location.line, text.lines().count());
        assert!(e.message.contains("A ->"));
        assert!(!e.notes.is_empty());
    }
    assert!(run("#define A m!(A)\nA").is_ok());
    assert_eq!(
        run("#define A A\n#if 0 && A\nx\n#endif").unwrap_err().kind,
        ErrorKind::Recursion
    );
}

#[test]
fn conditionals_expression_grammar_and_lazy_evaluation() {
    let expressions = [
        "1",
        "0xff == 255",
        "0b10 < 0o7",
        "1 || 0 && 0",
        "!(0) && +2 >= 1",
        "-9223372036854775808 < 0",
        "0 && -(-9223372036854775808) || 1",
        "MISSING == 0",
    ];
    for e in expressions {
        let o = run(&format!("#if {e}\nyes\n#else\nno\n#endif\n")).unwrap();
        assert!(o.text.contains("yes"), "{e}");
        assert!(!o.text.contains("no"));
    }
    let o=run("#define A 1\n#if defined A && defined(A) && !defined(B)\nyes\n#elif BAD(\nno\n#else\nno2\n#endif").unwrap();
    assert!(o.text.contains("yes"));
    for e in [
        "1 + 2",
        "1 & 2",
        "1.0",
        "1u32",
        "1 ==",
        "defined()",
        "(1",
        "9223372036854775808",
        "-(-9223372036854775808)",
    ] {
        assert!(run(&format!("#if {e}\nx\n#endif")).is_err(), "{e}");
    }
}

#[test]
fn inactive_directives_have_no_side_effects() {
    let o=run("#if 0\n#define F(x) broken\n#include \"missing\"\n#error don't interpret \"this\n#unknown\n#if BAD(\nx\n#endif\n#else\nyes\n#endif\n#ifdef F\nno\n#endif").unwrap();
    assert!(o.text.contains("yes"));
    assert!(!o.text.contains("no"));
    assert_eq!(o.dependencies.len(), 1);
    // Ordinary skipped code still must be lexically traversable.
    assert_eq!(
        run("#if 0\n\"unterminated\n#endif").unwrap_err().kind,
        ErrorKind::Lexical
    );
}

#[test]
fn conditional_structure_is_checked_even_when_inactive() {
    for text in [
        "#else",
        "#endif",
        "#elif 1",
        "#if 0\n#else\n#else\n#endif",
        "#if 1\n#else\n#elif 1\n#endif",
        "#if 1\n#endif extra",
        "#if 0\n#ifdef X Y\n#endif\n#endif",
        "#if 1\nx",
    ] {
        assert!(run(text).is_err(), "{text}");
    }
}

#[test]
fn includes_share_macros_but_not_conditional_stacks() {
    let s = inputs(&[
        (
            "root.rs",
            "#include \"a/first.rph\"\nVALUE\n#include \"a/first.rph\"",
        ),
        (
            "a/first.rph",
            "#ifndef VALUE\n#define VALUE 3\n#endif\n#include \"second.rph\"",
        ),
        ("a/second.rph", "VALUE;\n"),
    ]);
    let o = preprocess(&s, "root.rs", &Options::default()).unwrap();
    assert_eq!(o.dependencies.len(), 3);
    assert_eq!(o.text.matches('3').count(), 3);
    assert!(o.mappings.iter().any(|m| m.include_stack.len() == 2));
    let s = inputs(&[
        ("root.rs", "#if 1\n#include \"close\"\n#endif"),
        ("close", "#endif"),
    ]);
    let e = preprocess(&s, "root.rs", &Options::default()).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Conditional);
    assert_eq!(e.include_stack.len(), 1);
}

#[test]
fn include_search_order_cycles_and_root_escapes() {
    let s = inputs(&[
        ("src/root.rs", "#include \"value.rph\""),
        ("src/value.rph", "local"),
        ("inc/value.rph", "search"),
    ]);
    let mut options = Options::default();
    options.include_dirs.push("inc".into());
    assert!(
        preprocess(&s, "src/root.rs", &options)
            .unwrap()
            .text
            .contains("local")
    );
    let s = inputs(&[
        ("root.rs", "#include \"value.rph\""),
        ("inc/value.rph", "search"),
    ]);
    assert!(
        preprocess(&s, "root.rs", &options)
            .unwrap()
            .text
            .contains("search")
    );
    for text in [
        "#include \"root.rs\"",
        "#include \"../escape\"",
        "#include \"/absolute\"",
        "#include NAME",
        "#include \"missing\"",
    ] {
        assert_eq!(run(text).unwrap_err().kind, ErrorKind::Include);
    }
}

#[test]
fn original_locations_and_provenance_are_deterministic() {
    let s = inputs(&[
        ("root.rs", "#include \"é/fragment\"\r\n"),
        ("é/fragment", "// 🦀\r\n#define X X\r\n  X\r\n"),
    ]);
    let e = preprocess(&s, "root.rs", &Options::default()).unwrap_err();
    assert_eq!(e.location.source, SourceName::File("é/fragment".into()));
    assert_eq!((e.location.line, e.location.column), (3, 3));
    assert_eq!(e.include_stack.len(), 1);
    assert_eq!(
        e,
        preprocess(&s, "root.rs", &Options::default()).unwrap_err()
    );
    let a = run("#define X 3\nlet x=X;").unwrap();
    let b = run("#define X 3\nlet x=X;").unwrap();
    assert_eq!(a.text, b.text);
    assert_eq!(a.mappings, b.mappings);
    assert_eq!(a.dependencies, b.dependencies);
    let mut at = 0;
    for m in a.mappings {
        assert_eq!(m.generated.start, at);
        at = m.generated.end;
    }
    assert_eq!(at, a.text.len());
}

#[test]
fn predefined_configuration_is_ordered_and_calls_are_independent() {
    let s = inputs(&[("root.rs", "#ifdef FLAG\nFLAG\n#else\nabsent\n#endif")]);
    let mut o = Options::default();
    o.definitions = vec![
        InitialDefinition::Define("FLAG".into(), "2".into()),
        InitialDefinition::Undef("FLAG".into()),
        InitialDefinition::Define("FLAG".into(), "3".into()),
    ];
    assert!(preprocess(&s, "root.rs", &o).unwrap().text.contains('3'));
    assert!(
        preprocess(&s, "root.rs", &Options::default())
            .unwrap()
            .text
            .contains("absent")
    );
}

#[test]
fn errors_directives_and_fragments_never_execute_new_directives() {
    let e = run("#error can't parse \"this as Rust").unwrap_err();
    assert_eq!(e.kind, ErrorKind::ExplicitError);
    assert!(e.message.contains("can't"));
    assert_eq!(run("#unknown arg").unwrap_err().kind, ErrorKind::Directive);
    assert_eq!(
        run("#define X r#\"unterminated\nX").unwrap_err().kind,
        ErrorKind::Lexical
    );
    let o = run("#define HASH #\nHASH error message").unwrap();
    assert!(o.text.contains("#"));
}

#[test]
fn resource_limits_and_declaration_validation() {
    let mut text = String::new();
    for i in 0..130 {
        text += &format!("#define N{i} N{}\n", i + 1);
    }
    text += "N0";
    assert_eq!(run(&text).unwrap_err().kind, ErrorKind::Limit);
    let text = format!("{}{}", "#if 1\n".repeat(257), "#endif\n".repeat(257));
    assert_eq!(run(&text).unwrap_err().kind, ErrorKind::Limit);
    let mut s = Sources::default();
    assert!(s.insert("../bad", "".into()).is_err());
    s.insert("a/../x", "".into()).unwrap();
    assert!(s.insert("x", "".into()).is_err());
    let mut s = Sources::default();
    for i in 0..129 {
        s.insert(format!("f{i}"), format!("#include \"f{}\"", i + 1))
            .unwrap();
    }
    assert_eq!(
        preprocess(&s, "f0", &Options::default()).unwrap_err().kind,
        ErrorKind::Limit
    );
    assert!(
        run(&format!(
            "#if {}1{}\nx\n#endif",
            "(".repeat(300),
            ")".repeat(300)
        ))
        .is_err()
    );
    let s = inputs(&[("root.rs", "#define X 12345678\nX")]);
    assert_eq!(
        engine::with_limit(&s, Path::new("root.rs"), &Options::default(), 24)
            .unwrap_err()
            .kind,
        ErrorKind::Limit
    );
    assert!(engine::with_limit(&s, Path::new("root.rs"), &Options::default(), 64).is_ok());
}

#[test]
fn expression_error_columns_are_original() {
    let e = run("// 🦀\r\n#if  9223372036854775808\r\nx\r\n#endif").unwrap_err();
    assert_eq!((e.location.line, e.location.column), (2, 6));
    let e = run("#if -(-9223372036854775808)\nx\n#endif").unwrap_err();
    assert_eq!(e.location.column, 5);
}

#[test]
fn include_metadata_and_normalized_search_directories() {
    let s = inputs(&[
        ("src/root.rs", "#include \"fragment\"\nVALUE"),
        (
            "fragment",
            "\u{feff}#!/not/executed\r\n#define VALUE 6\r\nVALUE;",
        ),
    ]);
    for dir in [".", "unused/.."] {
        let mut options = Options::default();
        options.include_dirs.push(dir.into());
        let o = preprocess(&s, "src/root.rs", &options).unwrap();
        assert!(!o.text.contains('\u{feff}'));
        assert!(!o.text.contains("#!/"));
        assert_eq!(o.text.matches('6').count(), 2);
        assert!(o.text.contains("\r\n"));
    }
    let mut options = Options::default();
    options.include_dirs.push("../escape".into());
    assert_eq!(
        preprocess(&s, "src/root.rs", &options).unwrap_err().kind,
        ErrorKind::InvalidInput
    );
}

#[test]
fn comparison_matrix_and_flat_expression_chains() {
    for a in -2..=2 {
        for b in -2..=2 {
            for (op, yes) in [
                ("==", a == b),
                ("!=", a != b),
                ("<", a < b),
                ("<=", a <= b),
                (">", a > b),
                (">=", a >= b),
                ("&&", a != 0 && b != 0),
                ("||", a != 0 || b != 0),
            ] {
                let expr = format!("{a} {op} {b}");
                let value = expression::evaluate(&expr, Edition::Rust2024, &|_| false).unwrap();
                assert_eq!(value, i64::from(yes), "{expr}");
            }
        }
    }
    let chain = std::iter::repeat("1")
        .take(2000)
        .collect::<Vec<_>>()
        .join(" && ");
    assert_eq!(
        expression::evaluate(&chain, Edition::Rust2024, &|_| false).unwrap(),
        1
    );
    assert!(run("#if 1 || (2 + 3)\nx\n#endif").is_err());
    assert!(run("#if 0 && defined()\nx\n#endif").is_err());
}

#[test]
fn definitions_keep_internal_spacing_and_enforce_ascii_names() {
    assert!(run("#define X 1 /* x */\n#define X 1 // y\nX").is_ok());
    assert!(run("#define X 1 + 2\n#define X 1  + 2").is_err());
    for text in [
        "#define café 1",
        "#define r#X 1",
        "#undef X Y",
        "#define X \\",
    ] {
        assert!(run(text).is_err(), "{text}");
    }
    let o = run("#define X 9\nX::Y; Xsuffix; 1X; r#X; 'X;").unwrap();
    assert!(o.text.contains(" 9 ::Y; Xsuffix; 1X; r#X; 'X;"));
}
