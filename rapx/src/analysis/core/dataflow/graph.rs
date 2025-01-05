use std::cell::Cell;
use std::collections::HashSet;

use rustc_hir::def_id::DefId;
use rustc_index::IndexVec;
use rustc_middle::mir::{
    AggregateKind, BorrowKind, Const, Local, Operand, Place, PlaceElem, Rvalue, Statement,
    StatementKind, Terminator, TerminatorKind,
};
use rustc_middle::ty::TyKind;
use rustc_span::{Span, DUMMY_SP};

#[derive(Clone, Debug)]
pub enum NodeOp {
    //warning: the fields are related to the version of the backend rustc version
    Nop,
    Err,
    Const(String),
    //Rvalue
    Use,
    Repeat,
    Ref,
    ThreadLocalRef,
    AddressOf,
    Len,
    Cast,
    BinaryOp,
    CheckedBinaryOp,
    NullaryOp,
    UnaryOp,
    Discriminant,
    Aggregate(AggKind),
    ShallowInitBox,
    CopyForDeref,
    //TerminatorKind
    Call(DefId),
    CallOperand, // the first in_edge is the func
}

#[derive(Clone, Debug)]
pub enum EdgeOp {
    Nop,
    //Operand
    Move,
    Copy,
    Const,
    //Mutability
    Immut,
    Mut,
    //Place
    Deref,
    Field(String),
    Downcast(String),
    Index,
    ConstIndex,
    SubSlice,
}

#[derive(Clone)]
pub struct GraphEdge {
    pub src: Local,
    pub dst: Local,
    pub op: EdgeOp,
    pub seq: u32,
}

#[derive(Clone)]
pub struct GraphNode {
    pub op: NodeOp,
    pub span: Span, //the corresponding code span
    pub seq: u32, //the sequence number, edges with the same seq number are added in the same batch within a statement or terminator
    pub out_edges: Vec<EdgeIdx>,
    pub in_edges: Vec<EdgeIdx>,
}

impl GraphNode {
    pub fn new() -> Self {
        Self {
            op: NodeOp::Nop,
            span: DUMMY_SP,
            seq: 0,
            out_edges: vec![],
            in_edges: vec![],
        }
    }
}

pub type EdgeIdx = usize;
pub type GraphNodes = IndexVec<Local, GraphNode>;
pub type GraphEdges = IndexVec<EdgeIdx, GraphEdge>;
pub struct Graph {
    pub def_id: DefId,
    pub span: Span,
    pub argc: usize,
    pub nodes: GraphNodes, //constsis of locals in mir and newly created markers
    pub edges: GraphEdges,
    pub n_locals: usize,
}

impl Graph {
    pub fn new(def_id: DefId, span: Span, argc: usize, n_locals: usize) -> Self {
        Self {
            def_id,
            span,
            argc,
            nodes: GraphNodes::from_elem_n(GraphNode::new(), n_locals),
            edges: GraphEdges::new(),
            n_locals,
        }
    }

    pub fn add_node_edge(&mut self, src: Local, dst: Local, op: EdgeOp) -> EdgeIdx {
        let seq = self.nodes[dst].seq;
        let edge_idx = self.edges.push(GraphEdge { src, dst, op, seq });
        self.nodes[dst].in_edges.push(edge_idx);
        self.nodes[src].out_edges.push(edge_idx);
        edge_idx
    }

    pub fn add_const_edge(&mut self, src: String, dst: Local, op: EdgeOp) -> EdgeIdx {
        let seq = self.nodes[dst].seq;
        let mut const_node = GraphNode::new();
        const_node.op = NodeOp::Const(src);
        let src = self.nodes.push(const_node);
        let edge_idx = self.edges.push(GraphEdge { src, dst, op, seq });
        self.nodes[dst].in_edges.push(edge_idx);
        edge_idx
    }

    pub fn add_operand(&mut self, operand: &Operand, dst: Local) {
        match operand {
            Operand::Copy(place) => {
                let src = self.parse_place(place);
                self.add_node_edge(src, dst, EdgeOp::Copy);
            }
            Operand::Move(place) => {
                let src = self.parse_place(place);
                self.add_node_edge(src, dst, EdgeOp::Move);
            }
            Operand::Constant(boxed_const_op) => {
                self.add_const_edge(boxed_const_op.const_.to_string(), dst, EdgeOp::Const);
            }
        }
    }

    pub fn parse_place(&mut self, place: &Place) -> Local {
        fn parse_one_step(graph: &mut Graph, src: Local, place_elem: PlaceElem) -> Local {
            let dst = graph.nodes.push(GraphNode::new());
            match place_elem {
                PlaceElem::Deref => {
                    graph.add_node_edge(src, dst, EdgeOp::Deref);
                }
                PlaceElem::Field(field_idx, _) => {
                    graph.add_node_edge(src, dst, EdgeOp::Field(format!("{:?}", field_idx)));
                }
                PlaceElem::Downcast(symbol, _) => {
                    graph.add_node_edge(src, dst, EdgeOp::Downcast(symbol.unwrap().to_string()));
                }
                PlaceElem::Index(idx) => {
                    graph.add_node_edge(src, dst, EdgeOp::Index);
                    graph.add_node_edge(idx, dst, EdgeOp::Index); //Warning: no difference between src and idx in graph
                }
                PlaceElem::ConstantIndex { .. } => {
                    graph.add_node_edge(src, dst, EdgeOp::ConstIndex);
                }
                PlaceElem::Subslice { .. } => {
                    graph.add_node_edge(src, dst, EdgeOp::SubSlice);
                }
                _ => {
                    println!("{:?}", place_elem);
                    todo!()
                }
            }
            dst
        }
        let mut ret = place.local;
        for place_elem in place.projection {
            // if there are projections, then add marker nodes
            ret = parse_one_step(self, ret, place_elem);
        }
        ret
    }

    pub fn add_statm_to_graph(&mut self, statement: &Statement) {
        if let StatementKind::Assign(boxed_statm) = &statement.kind {
            let place = boxed_statm.0;
            let dst = self.parse_place(&place);
            self.nodes[dst].span = statement.source_info.span;
            let rvalue = &boxed_statm.1;
            match rvalue {
                Rvalue::Use(op) => {
                    self.add_operand(op, dst);
                    self.nodes[dst].op = NodeOp::Use;
                }
                Rvalue::Repeat(op, _) => {
                    self.add_operand(op, dst);
                    self.nodes[dst].op = NodeOp::Repeat;
                }
                Rvalue::Ref(_, borrow_kind, place) => {
                    let op = match borrow_kind {
                        BorrowKind::Shared => EdgeOp::Immut,
                        BorrowKind::Mut { .. } => EdgeOp::Mut,
                        BorrowKind::Fake(_) => EdgeOp::Nop, // todo
                    };
                    let src = self.parse_place(place);
                    self.add_node_edge(src, dst, op);
                    self.nodes[dst].op = NodeOp::Ref;
                }
                Rvalue::Len(place) => {
                    let src = self.parse_place(place);
                    self.add_node_edge(src, dst, EdgeOp::Nop);
                    self.nodes[dst].op = NodeOp::Len;
                }
                Rvalue::Cast(_cast_kind, operand, _) => {
                    self.add_operand(operand, dst);
                    self.nodes[dst].op = NodeOp::Cast;
                }
                Rvalue::BinaryOp(_, operands) => {
                    self.add_operand(&operands.0, dst);
                    self.add_operand(&operands.1, dst);
                    self.nodes[dst].op = NodeOp::CheckedBinaryOp;
                }
                Rvalue::Aggregate(boxed_kind, operands) => {
                    for operand in operands.iter() {
                        self.add_operand(operand, dst);
                    }
                    match **boxed_kind {
                        AggregateKind::Array(_) => {
                            self.nodes[dst].op = NodeOp::Aggregate(AggKind::Array)
                        }
                        AggregateKind::Tuple => {
                            self.nodes[dst].op = NodeOp::Aggregate(AggKind::Tuple)
                        }
                        AggregateKind::Adt(def_id, ..) => {
                            self.nodes[dst].op = NodeOp::Aggregate(AggKind::Adt(def_id))
                        }
                        AggregateKind::Closure(def_id, ..) => {
                            self.nodes[dst].op = NodeOp::Aggregate(AggKind::Closure(def_id))
                        }
                        _ => {
                            println!("{:?}", statement);
                            todo!()
                        }
                    }
                }
                Rvalue::UnaryOp(_, operand) => {
                    self.add_operand(operand, dst);
                    self.nodes[dst].op = NodeOp::UnaryOp;
                }
                Rvalue::NullaryOp(_, ty) => {
                    self.add_const_edge(ty.to_string(), dst, EdgeOp::Nop);
                    self.nodes[dst].op = NodeOp::NullaryOp;
                }
                Rvalue::ThreadLocalRef(_) => {
                    todo!()
                }
                Rvalue::Discriminant(place) => {
                    let src = self.parse_place(place);
                    self.add_node_edge(src, dst, EdgeOp::Nop);
                    self.nodes[dst].op = NodeOp::Discriminant;
                }
                Rvalue::ShallowInitBox(operand, _) => {
                    self.add_operand(operand, dst);
                    self.nodes[dst].op = NodeOp::ShallowInitBox;
                }
                Rvalue::CopyForDeref(place) => {
                    let src = self.parse_place(place);
                    self.add_node_edge(src, dst, EdgeOp::Nop);
                    self.nodes[dst].op = NodeOp::CopyForDeref;
                }
                Rvalue::RawPtr(_, _) => todo!(),
            };
            self.nodes[dst].seq += 1;
        }
    }

    pub fn add_terminator_to_graph(&mut self, terminator: &Terminator) {
        if let TerminatorKind::Call {
            func,
            args,
            destination,
            ..
        } = &terminator.kind
        {
            let dst = destination.local;
            match func {
                Operand::Constant(boxed_cnst) => {
                    if let Const::Val(_, ty) = boxed_cnst.const_ {
                        if let TyKind::FnDef(def_id, _) = ty.kind() {
                            for op in args.iter() {
                                //rustc version related
                                self.add_operand(&op.node, dst);
                            }
                            self.nodes[dst].op = NodeOp::Call(*def_id);
                        }
                    }
                },
                Operand::Move(_) => {
                    self.add_operand(func, dst); //the func is a place
                    for op in args.iter() {
                        //rustc version related
                        self.add_operand(&op.node, dst);
                    }
                    self.nodes[dst].op = NodeOp::CallOperand;
                },
                _ =>  {
                    println!("{:?}", func);
                    todo!();
                }
            }
            self.nodes[dst].span = terminator.source_info.span;
            self.nodes[dst].seq += 1;
        }
    }

    pub fn collect_equivalent_locals(&self, local: Local) -> HashSet<Local> {
        let mut set = HashSet::new();
        let mut root = local;
        let mut find_root_operator = |graph: &Graph, idx: Local| -> DFSStatus {
            let node = &graph.nodes[idx];
            match node.op {
                NodeOp::Nop | NodeOp::Use | NodeOp::Ref => {
                    //Nop means an orphan node or a parameter
                    root = idx;
                    DFSStatus::Continue
                }
                _ => DFSStatus::Stop,
            }
        };
        let mut find_equivalent_operator = |graph: &Graph, idx: Local| -> DFSStatus {
            let node = &graph.nodes[idx];
            if set.contains(&idx) {
                return DFSStatus::Stop;
            }
            match node.op {
                NodeOp::Nop | NodeOp::Use | NodeOp::Ref => {
                    set.insert(idx);
                    DFSStatus::Continue
                }
                _ => DFSStatus::Stop,
            }
        };
        // Algorithm: dfs along upside to find the root node, and then dfs along downside to collect equivalent locals
        self.dfs(
            local,
            Direction::Upside,
            &mut find_root_operator,
            &mut Self::equivalent_edge_validator,
            true,
        );
        self.dfs(
            root,
            Direction::Downside,
            &mut find_equivalent_operator,
            &mut Self::equivalent_edge_validator,
            true,
        );
        set
    }

    pub fn is_connected(&self, idx_1: Local, idx_2: Local) -> bool {
        let target = idx_2;
        let find = Cell::new(false);
        let mut node_operator = |_: &Graph, idx: Local| -> DFSStatus {
            find.set(idx == target);
            if find.get() {
                DFSStatus::Stop
            } else {
                // if not found, move on
                DFSStatus::Continue
            }
        };
        self.dfs(
            idx_1,
            Direction::Downside,
            &mut node_operator,
            &mut Self::always_true_edge_validator,
            false,
        );
        if !find.get() {
            self.dfs(
                idx_1,
                Direction::Upside,
                &mut node_operator,
                &mut Self::always_true_edge_validator,
                false,
            );
        }
        find.get()
    }

    // Whether there exists dataflow between each parameter and the return value
    pub fn param_return_deps(&self) -> IndexVec<Local, bool> {
        let _0 = Local::from_usize(0);
        let deps = (0..self.argc + 1) //the length is argc + 1, because _0 depends on _0 itself.
            .map(|i| {
                let _i = Local::from_usize(i);
                self.is_connected(_i, _0)
            })
            .collect();
        deps
    }

    // This function uses precedence traversal.
    // The node operator and edge validator decide how far the traversal can reach.
    // `traverse_all` decides if a branch finds the target successfully, whether the traversal will continue or not.
    // For example, if you need to instantly stop the traversal once finding a certain node, then set `traverse_all` to false.
    // If you want to traverse all the reachable nodes which are decided by the operator and validator, then set `traverse_all` to true.
    pub fn dfs<F, G>(
        &self,
        now: Local,
        direction: Direction,
        node_operator: &mut F,
        edge_validator: &mut G,
        traverse_all: bool,
    ) -> DFSStatus
    where
        F: FnMut(&Graph, Local) -> DFSStatus,
        G: FnMut(&Graph, EdgeIdx) -> DFSStatus,
    {
        macro_rules! traverse {
            ($edges: ident, $field: ident) => {
                for edge_idx in self.nodes[now].$edges.iter() {
                    let edge = &self.edges[*edge_idx];
                    if matches!(edge_validator(self, *edge_idx), DFSStatus::Continue) {
                        let result = self.dfs(
                            edge.$field,
                            direction,
                            node_operator,
                            edge_validator,
                            traverse_all,
                        );
                        if matches!(result, DFSStatus::Stop) && !traverse_all {
                            return DFSStatus::Stop;
                        }
                    }
                }
            };
        }

        if matches!(node_operator(self, now), DFSStatus::Continue) {
            match direction {
                Direction::Upside => {
                    traverse!(in_edges, src);
                }
                Direction::Downside => {
                    traverse!(out_edges, dst);
                }
                Direction::Both => {
                    traverse!(in_edges, src);
                    traverse!(out_edges, dst);
                }
            };
            DFSStatus::Continue
        } else {
            DFSStatus::Stop
        }
    }

    pub fn get_upside_idx(&self, node_idx: Local, order: usize) -> Option<Local> {
        if let Some(edge_idx) = self.nodes[node_idx].in_edges.get(order) {
            Some(self.edges[*edge_idx].src)
        } else {
            None
        }
    }

    pub fn get_downside_idx(&self, node_idx: Local, order: usize) -> Option<Local> {
        if let Some(edge_idx) = self.nodes[node_idx].out_edges.get(order) {
            Some(self.edges[*edge_idx].dst)
        } else {
            None
        }
    }
}

impl Graph {
    pub fn equivalent_edge_validator(graph: &Graph, idx: EdgeIdx) -> DFSStatus {
        match graph.edges[idx].op {
            EdgeOp::Copy | EdgeOp::Move | EdgeOp::Mut | EdgeOp::Immut => DFSStatus::Continue,
            EdgeOp::Nop
            | EdgeOp::Const
            | EdgeOp::Deref
            | EdgeOp::Downcast(_)
            | EdgeOp::Field(_)
            | EdgeOp::Index
            | EdgeOp::ConstIndex
            | EdgeOp::SubSlice => DFSStatus::Stop,
        }
    }

    pub fn always_true_edge_validator(_: &Graph, _: EdgeIdx) -> DFSStatus {
        DFSStatus::Continue
    }
}

#[derive(Clone, Copy)]
pub enum Direction {
    Upside,
    Downside,
    Both,
}

pub enum DFSStatus {
    Continue,
    Stop,
}

#[derive(Clone, Copy, Debug)]
pub enum AggKind {
    Array,
    Tuple,
    Adt(DefId),
    Closure(DefId),
}
