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
