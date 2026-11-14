//! The engine's re-exports must name the core types, never copies of them.

use std::any::TypeId;

macro_rules! same_type {
    ($engine:path, $core:path) => {
        assert_eq!(
            TypeId::of::<$engine>(),
            TypeId::of::<$core>(),
            concat!(stringify!($engine), " is not ", stringify!($core))
        );
    };
}

#[test]
fn every_re_export_names_the_core_type() {
    same_type!(er_engine::git::DiffFile, er_core::diff::DiffFile);
    same_type!(er_engine::git::DiffHunk, er_core::diff::DiffHunk);
    same_type!(er_engine::git::DiffLine, er_core::diff::DiffLine);
    same_type!(er_engine::git::LineType, er_core::diff::LineType);
    same_type!(er_engine::git::FileStatus, er_core::status::FileStatus);
    same_type!(er_git::FileStatus, er_core::status::FileStatus);
    same_type!(er_store::ExpertId, er_core::slot::ExpertId);
    same_type!(er_store::DiagramId, er_core::slot::DiagramId);
    same_type!(er_engine::ai::ErReview, er_core::review::ErReview);
    same_type!(er_engine::ai::ErFileReview, er_core::review::ErFileReview);
    same_type!(er_engine::ai::Finding, er_core::review::Finding);
    same_type!(er_engine::ai::Confidence, er_core::review::Confidence);
    same_type!(er_engine::ai::RiskLevel, er_core::review::RiskLevel);
    same_type!(er_engine::ai::EvidenceItem, er_core::review::EvidenceItem);
    same_type!(er_engine::ai::InlineLayers, er_core::review::InlineLayers);
    same_type!(er_engine::ai::AiResponse, er_core::review::AiResponse);
    same_type!(
        er_engine::ai::ErQuestions,
        er_core::annotations::ErQuestions
    );
    same_type!(er_engine::ai::ErNotes, er_core::annotations::ErNotes);
    same_type!(
        er_engine::ai::PrivateAnnotation,
        er_core::annotations::PrivateAnnotation
    );
    same_type!(
        er_engine::ai::ErGithubComments,
        er_core::annotations::ErGithubComments
    );
    same_type!(
        er_engine::ai::GithubComment,
        er_core::annotations::GithubComment
    );
    same_type!(
        er_engine::ai::GitHubSyncState,
        er_core::annotations::GitHubSyncState
    );
    same_type!(
        er_engine::ai::ArbiterReview,
        er_core::arbiter::ArbiterReview
    );
    same_type!(
        er_engine::ai::ArbiterRuling,
        er_core::arbiter::ArbiterRuling
    );
    same_type!(
        er_engine::ai::ArbiterVerdict,
        er_core::arbiter::ArbiterVerdict
    );
    same_type!(
        er_engine::ai::ArbiterEffect,
        er_core::arbiter::ArbiterEffect
    );
    same_type!(
        er_engine::ai::ValidationEffect,
        er_core::validation::ValidationEffect
    );
    same_type!(
        er_engine::ai::CommentAnchor,
        er_core::relocate::CommentAnchor
    );
    same_type!(
        er_engine::ai::RelocationResult,
        er_core::relocate::RelocationResult
    );
}

fn assert_core_artifact<T: er_core::artifact::Artifact>() {}
fn assert_core_slot<T: er_core::slot::SlotName>() {}

#[test]
fn the_store_re_exports_the_core_traits() {
    // `ErTour`'s impl is written against `er_store::Artifact`; it only satisfies
    // the core bound if the store's re-export names the same trait.
    assert_core_artifact::<er_engine::ai::ErTour>();
    assert_core_slot::<er_store::ExpertId>();
}
