#![allow(
    clippy::assigning_clones,
    clippy::missing_errors_doc,
    clippy::must_use_candidate,
    clippy::needless_pass_by_value,
    clippy::needless_raw_string_hashes,
    clippy::single_match_else,
    clippy::too_many_lines
)]
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

pub mod account_worker;
pub mod adapters;
pub mod domain;
pub mod error;
pub mod nucleus;
pub mod seed;
pub mod store;
pub mod worker;

pub use error::{Error, Result};
