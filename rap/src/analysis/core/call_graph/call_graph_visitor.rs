use super::call_graph_helper::CallGraphInfo;
use rustc_hir::def_id::DefId;
use rustc_middle::mir;
use rustc_middle::ty::{FnDef, TyCtxt};

pub struct CallGraphVisitor<'b, 'tcx> {
    tcx: TyCtxt<'tcx>,
    def_id: DefId,
    body: &'tcx mir::Body<'tcx>,
    call_graph_info: &'b mut CallGraphInfo,
}

impl<'b, 'tcx> CallGraphVisitor<'b, 'tcx> {
    pub fn new(
        tcx: TyCtxt<'tcx>,
        def_id: DefId,
        body: &'tcx mir::Body<'tcx>,
        call_graph_info: &'b mut CallGraphInfo,
    ) -> Self {
        Self {
            tcx: tcx,
            def_id: def_id,
            body: body,
            call_graph_info: call_graph_info,
        }
    }

    pub fn add_in_call_graph(
        &mut self,
        caller_def_path: &String,
        callee_def_id: DefId,
        callee_def_path: &String,
    ) {
        if let Some(caller_id) = self.call_graph_info.get_noed_by_path(caller_def_path) {
            if let Some(callee_id) = self.call_graph_info.get_noed_by_path(callee_def_path) {
                self.call_graph_info
                    .add_funciton_call_edge(caller_id, callee_id);
            } else {
                self.call_graph_info
                    .add_node(callee_def_id, callee_def_path);
                if let Some(callee_id) = self.call_graph_info.get_noed_by_path(callee_def_path) {
                    self.call_graph_info
                        .add_funciton_call_edge(caller_id, callee_id);
                }
            }
        }
    }

    pub fn visit(&mut self) {
        let caller_path_str = self.tcx.def_path_str(self.def_id);
        self.call_graph_info.add_node(self.def_id, &caller_path_str);
        for (_, data) in self.body.basic_blocks.iter().enumerate() {
            let terminator = data.terminator();
            self.visit_terminator(&terminator);
        }
    }

    fn add_to_call_graph(&mut self, callee_def_id: DefId) {
        let caller_def_path = self.tcx.def_path_str(self.def_id);
        let callee_def_path = self.tcx.def_path_str(callee_def_id);
        // let callee_location = self.tcx.def_span(callee_def_id);
        if callee_def_id == self.def_id {
            // Recursion
            println!("Warning! Find a recursion function which may cause stackoverflow!")
        }
        self.add_in_call_graph(&caller_def_path, callee_def_id, &callee_def_path);
        println!("")
    }

    fn visit_terminator(&mut self, terminator: &mir::Terminator<'tcx>) {
        if let mir::TerminatorKind::Call { func, .. } = &terminator.kind {
            if let mir::Operand::Constant(constant) = func {
                if let FnDef(callee_def_id, _) = constant.const_.ty().kind() {
                    self.add_to_call_graph(*callee_def_id);
                }
            }
        }
    }
}
