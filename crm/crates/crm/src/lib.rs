#![allow(
    clippy::missing_errors_doc,
    clippy::must_use_candidate,
    clippy::needless_pass_by_value,
    clippy::too_many_lines
)]
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

pub mod api;
#[doc(hidden)]
pub mod app;
#[doc(hidden)]
pub mod cli;
#[doc(hidden)]
pub mod error;
mod maintenance;
#[doc(hidden)]
pub mod model;
#[doc(hidden)]
pub mod nucleus;
#[doc(hidden)]
pub mod store;
#[doc(hidden)]
pub mod worker;

pub use error::{Error, Result};

pub fn main_entry() -> i32 {
    app::main_entry()
}
