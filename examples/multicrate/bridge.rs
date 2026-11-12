#[unsafe(no_mangle)] pub extern "C" fn answer() -> i32 { middle::value() }
#[unsafe(no_mangle)] pub extern "C" fn build_mode() -> i32 { middle::build_mode() }
