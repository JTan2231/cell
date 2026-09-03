#![allow(
    clippy::missing_errors_doc,
    clippy::must_use_candidate,
    clippy::needless_pass_by_value,
    clippy::too_many_lines
)]
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

pub mod app;
pub mod cli;
pub mod error;
pub mod model;
pub mod nucleus;
pub mod store;
pub mod worker;

pub use error::{Error, Result};

pub fn main_entry() -> i32 {
    app::main_entry()
}
