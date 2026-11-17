//! Core library for Mikoshi: KDBX access and domain logic, with no dependency
//! on the desktop shell so it can be tested on its own.

pub mod model;
pub mod search;
pub mod secret;

pub use secret::Secret;
