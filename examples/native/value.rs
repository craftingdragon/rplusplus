#![no_std]
mod sub;
#[unsafe(no_mangle)] pub extern "C" fn rust_value() -> i32 { sub::VALUE }
