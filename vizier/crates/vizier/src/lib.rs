#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::too_many_arguments,
    clippy::too_many_lines
)]

pub mod app;
pub mod contracts;
pub mod error;
pub mod git;
pub mod model;
pub mod nucleus;
pub mod store;
pub mod workflow;

pub use app::run;
pub use error::{AppError, AppResult};
