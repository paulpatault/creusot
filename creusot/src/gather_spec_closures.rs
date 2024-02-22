use indexmap::{IndexMap, IndexSet};

use crate::{
    ctx::TranslationCtx,
    pearlite::Term,
    util::{self, ghost_closure_id},
};
use rustc_data_structures::graph::WithSuccessors;
use rustc_hir::def_id::DefId;
use rustc_middle::{
    mir::{visit::Visitor, AggregateKind, BasicBlock, Body, Location, Operand, Rvalue},
    ty::TyCtxt,
};

#[derive(Debug, Clone, Copy)]
pub enum LoopSpecKind {
    Invariant,
    Variant,
}

pub(crate) fn assertions_and_ghosts<'tcx>(
    ctx: &mut TranslationCtx<'tcx>,
    body: &Body<'tcx>,
) -> IndexMap<DefId, Term<'tcx>> {
    let mut visitor = Closures::new(ctx.tcx);
    visitor.visit_body(&body);

    let mut assertions: IndexMap<_, _> = Default::default();
    for clos in visitor.closures.into_iter() {
        if util::is_assertion(ctx.tcx, clos) {
            let term = ctx.term(clos).unwrap().clone();
            assertions.insert(clos, term);
        } else if util::is_ghost_closure(ctx.tcx, clos) {
            let term = ctx.term(clos).unwrap().clone();
            // A hack should probably be separately tracked
            assertions.insert(clos, term);
        }
    }
    assertions
}

// Collect the closures in thir, so that we can do typechecking ourselves, and
// translate the invariant closure from thir.
struct Closures<'tcx> {
    pub tcx: TyCtxt<'tcx>,
    pub closures: IndexSet<DefId>,
}

impl<'tcx> Closures<'tcx> {
    fn new(tcx: TyCtxt<'tcx>) -> Self {
        Closures { tcx, closures: IndexSet::new() }
    }
}

impl<'tcx> Visitor<'tcx> for Closures<'tcx> {
    fn visit_rvalue(&mut self, rvalue: &Rvalue<'tcx>, loc: Location) {
        match rvalue {
            Rvalue::Aggregate(box AggregateKind::Closure(id, _), _) => {
                self.closures.insert(*id);
            }
            Rvalue::Use(Operand::Constant(box ck)) => {
                if let Some(def_id) = ghost_closure_id(self.tcx, ck.const_.ty()) {
                    self.closures.insert(def_id);
                }
            }
            _ => {}
        }
        self.super_rvalue(rvalue, loc);
    }
}

struct Invariants<'tcx> {
    tcx: TyCtxt<'tcx>,
    invariants: IndexMap<Location, (DefId, LoopSpecKind)>,
}

impl<'tcx> Visitor<'tcx> for Invariants<'tcx> {
    fn visit_rvalue(&mut self, rvalue: &Rvalue<'tcx>, loc: Location) {
        if let Rvalue::Aggregate(box AggregateKind::Closure(id, _), _) = rvalue {
            let kind = if util::is_invariant(self.tcx, *id) {
                LoopSpecKind::Invariant
            } else if util::is_loop_variant(self.tcx, *id) {
                LoopSpecKind::Variant
            } else {
                return;
            };
            self.invariants.insert(loc, (*id, kind));
        }
        self.super_rvalue(rvalue, loc);
    }
}

// Calculate the *actual* location of invariants in MIR
pub(crate) fn corrected_invariant_names_and_locations<'tcx>(
    ctx: &mut TranslationCtx<'tcx>,
    body: &Body<'tcx>,
) -> IndexMap<BasicBlock, Vec<(LoopSpecKind, Term<'tcx>)>> {
    let mut results = IndexMap::new();

    let mut invs_gather = Invariants { tcx: ctx.tcx, invariants: IndexMap::new() };
    invs_gather.visit_body(body);

    for (loc, (clos, kind)) in invs_gather.invariants.into_iter() {
        let mut target: BasicBlock = loc.block;

        loop {
            let mut succs = body.basic_blocks.successors(target);

            target = succs.next().unwrap();

            // Check if `taget_block` is a loop header by testing if it dominates
            // one of its predecessors.
            if let Some(preds) = body.basic_blocks.predecessors().get(target) {
                let is_loop_header = preds
                    .iter()
                    .any(|pred| body.basic_blocks.dominators().dominates(target, *pred));

                if is_loop_header {
                    break;
                }
            };

            // If we've hit a switch then stop trying to push the invariants down.
            if body[target].terminator().kind.as_switch().is_some() {
                panic!("Could not find loop header")
            }
        }

        let term = ctx.term(clos).unwrap().clone();
        results.entry(target).or_insert_with(Vec::new).push((kind, term));
    }

    results
}
