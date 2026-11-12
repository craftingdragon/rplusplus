// `gen` is legal in edition 2021 and reserved in edition 2024.
pub fn value() -> i32 { let gen = vec![20, 20]; gen.into_iter().sum() }
pub fn build_mode() -> i32 { if cfg!(debug_assertions) { 1 } else { 0 } }
