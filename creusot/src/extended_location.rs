use rustc_middle::mir::Location;
use rustc_mir_dataflow::{self as dataflow, Analysis, ResultsCursor};

// Dataflow locations
#[derive(Debug, Copy, Clone)]
pub enum ExtendedLocation {
    Start(Location),
    Mid(Location),
}

// Rust hides the real `Direction` trait from me, this hack recreates it
pub trait Dir {
    fn is_forward() -> bool;
}

impl Dir for dataflow::Forward {
    fn is_forward() -> bool {
        true
    }
}

impl Dir for dataflow::Backward {
    fn is_forward() -> bool {
        false
    }
}

impl ExtendedLocation {
    pub(crate) fn seek_to<'tcx, A, D>(self, cursor: &mut ResultsCursor<'_, 'tcx, A>)
    where
        A: Analysis<'tcx, Direction = D>,
        D: Dir,
    {
        use ExtendedLocation::*;
        if D::is_forward() {
            match self {
                Start(loc) => cursor.seek_before_primary_effect(loc),
                Mid(loc) => cursor.seek_after_primary_effect(loc),
            }
        } else {
            match self {
                Start(loc) => cursor.seek_after_primary_effect(loc),
                Mid(loc) => cursor.seek_before_primary_effect(loc),
            }
        }
    }
}
