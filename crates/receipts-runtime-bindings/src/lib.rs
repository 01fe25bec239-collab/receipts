//! Physical composition only; this crate owns none of the composed capsule semantics.

mod capsule_family;

pub use capsule_family::RuntimeCapsuleFamily;

#[cfg(test)]
mod capsule_family_tests;
