
#[unsafe(no_mangle)] pub extern "C" fn extra_value() -> i32 { rustpart::rust_value() - 38 }
