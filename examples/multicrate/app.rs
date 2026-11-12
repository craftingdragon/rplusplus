#[cfg(not(driver_flag))] compile_error!("Rust flags were not propagated");
unsafe extern "C" { fn cpp_value() -> i32; }
fn main() { assert_eq!(middle::value(), unsafe { cpp_value() }); }
