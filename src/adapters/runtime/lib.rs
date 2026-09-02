mod adapter;
mod auth_status;
mod failure;

#[cfg(test)]
mod adapter_tests;
#[cfg(test)]
mod conformance_tests;

pub use adapter::RuntimeAdapter;
pub use auth_status::RuntimeAuthStatus;
pub use failure::FailureClass;
