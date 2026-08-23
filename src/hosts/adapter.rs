//! The [`HostAdapter`] translation boundary.
//!
//! Each method names exactly one semantic operation of the frozen Host
//! Integration architecture. The operations are declared here, not
//! implemented: this slice provides no concrete host behavior of any kind.
//!
//! Every input or result whose real contract is owned by another slice
//! appears as an unbound associated type below. Those placeholders have no
//! fields, no constructors, and no semantics in this crate; they exist so
//! the interface compiles today without pretending the externally owned
//! frozen contracts already exist. Binding them is the job of the slices
//! that own those contracts, never of this boundary.

use std::future::Future;

use crate::host_id::HostId;

/// Translation boundary between the Receipts core and one external host.
///
/// Implementations translate core intent into host-native interaction and
/// host-native observations back into core-facing terms. Declaring this
/// trait grants nothing: `detect` probes nothing, `install` installs
/// nothing, `start` launches nothing, `emit` normalizes nothing, and
/// `capabilities` claims nothing. This interface fixes vocabulary only;
/// concrete adapters must earn their authority from the slices that bind
/// the placeholder contracts.
pub trait HostAdapter {
    /// Unbound placeholder for the detection outcome.
    type DetectOutcome;

    /// Unbound placeholder for the externally owned frozen `InstallPlan`
    /// contract.
    type InstallPlan;

    /// Unbound placeholder for the outcome of translating an install plan.
    type InstallOutcome;

    /// Unbound placeholder for the externally owned frozen `CoreHandle`
    /// contract.
    type CoreHandle;

    /// Unbound placeholder for the externally owned frozen
    /// `NormalizedHostEvent` contract.
    type NormalizedHostEvent;

    /// Unbound placeholder for the outcome of forwarding one normalized
    /// host event.
    type EmitOutcome;

    /// Unbound placeholder for the externally owned frozen `CoreView`
    /// contract.
    type CoreView;

    /// Unbound placeholder for the outcome of presenting a core view.
    type PresentOutcome;

    /// Unbound placeholder for the externally owned frozen `UserPrompt`
    /// contract.
    type UserPrompt;

    /// Unbound placeholder for the externally owned frozen `UserResponse`
    /// contract.
    type UserResponse;

    /// Pending completion of one [`request_user_input`](Self::request_user_input)
    /// call, built with `std`/`core` [`Future`] facilities only.
    type UserInputPending: Future<Output = Self::UserResponse>;

    /// Unbound placeholder for the externally owned frozen
    /// `HostCapabilityReport` contract.
    type HostCapabilityReport;

    /// Unbound placeholder for the externally owned frozen shutdown-reason
    /// contract.
    type ShutdownReason;

    /// Unbound placeholder for the outcome of a completed shutdown.
    type ShutdownOutcome;

    /// Reports which host identity this adapter translates for.
    fn id(&self) -> HostId;

    /// Semantic operation: determine whether this adapter's host applies.
    ///
    /// Declared only; implementations must not probe the environment, the
    /// process tree, or the filesystem on its behalf.
    fn detect(&self) -> Self::DetectOutcome;

    /// Semantic operation: translate a plan for making the host available.
    ///
    /// Declared only; no installation behavior exists in this slice.
    fn install(&self, plan: &Self::InstallPlan) -> Self::InstallOutcome;

    /// Semantic operation: start the host session and expose its handle.
    ///
    /// Declared only; no process control exists in this slice.
    fn start(&self) -> Self::CoreHandle;

    /// Semantic operation: forward one normalized host event toward the
    /// core.
    ///
    /// Declared only; normalization is not performed at this boundary.
    fn emit(&self, event: &Self::NormalizedHostEvent) -> Self::EmitOutcome;

    /// Semantic operation: present a core view to the user through the
    /// host.
    ///
    /// Declared only; no rendering exists in this slice.
    fn present(&self, view: &Self::CoreView) -> Self::PresentOutcome;

    /// Semantic operation: request input from the user through the host,
    /// completing asynchronously with the user's response.
    ///
    /// Declared only; prompts are translated verbatim by implementations,
    /// never invented or normalized here.
    fn request_user_input(&mut self, prompt: Self::UserPrompt) -> Self::UserInputPending;

    /// Semantic operation: report what the host can do.
    ///
    /// Declared only; no capability probing or claims exist in this slice.
    fn capabilities(&self) -> Self::HostCapabilityReport;

    /// Semantic operation: shut the host session down for the given reason.
    ///
    /// Declared only; no process control exists in this slice. Consuming
    /// `self` marks shutdown as terminal: no further operations are
    /// representable afterwards.
    fn shutdown(self, reason: Self::ShutdownReason) -> Self::ShutdownOutcome
    where
        Self: Sized;
}
