//! Review Integration: closed vocabularies and a non-temporal structured core.

pub mod a4_review_vocabulary;

pub use a4_review_vocabulary::{
    A4ReviewDimension, A4ReviewDimensionAssessment, A4ReviewFindingCategory,
    A4ReviewFindingConfidence, A4ReviewFindingSeverity, A4ReviewFindingSource,
    A4ReviewRecommendedAction, A4ReviewVerdict,
};

#[cfg(test)]
mod a4_review_vocabulary_tests;

pub mod a4_review_structured_core;
pub use a4_review_structured_core::{
    A4ReviewConstructionError, A4ReviewDimensionReview, A4ReviewFinding, A4ReviewNonTemporalCore,
    A4ReviewReproduction, A4ReviewReproductionCheck, A4ReviewReproductionCheckResult,
    A4ReviewReproductionLimitation, A4ReviewReviewer,
};

#[cfg(test)]
mod a4_review_structured_core_tests;
