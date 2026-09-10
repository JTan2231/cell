//! Stateless collection and rendering of Cell operational observations.

pub mod installation;
pub mod probe;
pub mod render;
pub mod report;

pub use report::{Collection, CollectionOptions, collect};
