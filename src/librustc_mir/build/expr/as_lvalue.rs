// Copyright 2015 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! See docs in build/expr/mod.rs

use build::{BlockAnd, BlockAndExtension, Builder};
use build::expr::category::Category;
use hair::*;
use rustc::mir::*;

use rustc_data_structures::indexed_vec::Idx;

impl<'a, 'gcx, 'tcx> Builder<'a, 'gcx, 'tcx> {
    /// Compile `expr`, yielding an lvalue that we can move from etc.
    pub fn as_lvalue<M>(&mut self,
                        block: BasicBlock,
                        expr: M)
                        -> BlockAnd<Lvalue<'tcx>>
        where M: Mirror<'tcx, Output=Expr<'tcx>>
    {
        let expr = self.hir.mirror(expr);
        self.expr_as_lvalue(block, expr)
    }

    fn expr_as_lvalue(&mut self,
                      mut block: BasicBlock,
                      expr: Expr<'tcx>)
                      -> BlockAnd<Lvalue<'tcx>> {
        debug!("expr_as_lvalue(block={:?}, expr={:?})", block, expr);

        let this = self;
        let expr_span = expr.span;
        let source_info = this.source_info(expr_span);
        match expr.kind {
            ExprKind::Scope { region_scope, value } => {
                this.in_scope((region_scope, source_info), block, |this| {
                    this.as_lvalue(block, value)
                })
            }
            ExprKind::Field { lhs, name } => {
                let lvalue = unpack!(block = this.as_lvalue(block, lhs));
                let lvalue = lvalue.field(name, expr.ty);
                block.and(lvalue)
            }
            ExprKind::Deref { arg } => {
                let lvalue = unpack!(block = this.as_lvalue(block, arg));
                let lvalue = lvalue.deref();
                block.and(lvalue)
            }
            ExprKind::Index { lhs, index } => {
                let (usize_ty, bool_ty) = (this.hir.usize_ty(), this.hir.bool_ty());

                let slice = unpack!(block = this.as_lvalue(block, lhs));
                // region_scope=None so lvalue indexes live forever. They are scalars so they
                // do not need storage annotations, and they are often copied between
                // places.
                let idx = unpack!(block = this.as_temp(block, None, index));

                // bounds check:
                let (len, lt) = (this.temp(usize_ty.clone(), expr_span),
                                 this.temp(bool_ty, expr_span));
                this.cfg.push_assign(block, source_info, // len = len(slice)
                                     &len, Rvalue::Len(slice.clone()));
                this.cfg.push_assign(block, source_info, // lt = idx < len
                                     &lt, Rvalue::BinaryOp(BinOp::Lt,
                                                           Operand::Consume(Lvalue::Local(idx)),
                                                           Operand::Consume(len.clone())));

                let msg = AssertMessage::BoundsCheck {
                    len: Operand::Consume(len),
                    index: Operand::Consume(Lvalue::Local(idx))
                };
                let success = this.assert(block, Operand::Consume(lt), true,
                                          msg, expr_span);
                success.and(slice.index(idx))
            }
            ExprKind::SelfRef => {
                block.and(Lvalue::Local(Local::new(1)))
            }
            ExprKind::VarRef { id } => {
                let index = this.var_indices[&id];
                block.and(Lvalue::Local(index))
            }
            ExprKind::StaticRef { id } => {
                block.and(Lvalue::Static(Box::new(Static { def_id: id, ty: expr.ty })))
            }

            ExprKind::Array { .. } |
            ExprKind::Tuple { .. } |
            ExprKind::Adt { .. } |
            ExprKind::Closure { .. } |
            ExprKind::Unary { .. } |
            ExprKind::Binary { .. } |
            ExprKind::LogicalOp { .. } |
            ExprKind::Box { .. } |
            ExprKind::Cast { .. } |
            ExprKind::Use { .. } |
            ExprKind::NeverToAny { .. } |
            ExprKind::ReifyFnPointer { .. } |
            ExprKind::ClosureFnPointer { .. } |
            ExprKind::UnsafeFnPointer { .. } |
            ExprKind::Unsize { .. } |
            ExprKind::Repeat { .. } |
            ExprKind::Borrow { .. } |
            ExprKind::If { .. } |
            ExprKind::Match { .. } |
            ExprKind::Loop { .. } |
            ExprKind::Block { .. } |
            ExprKind::Assign { .. } |
            ExprKind::AssignOp { .. } |
            ExprKind::Break { .. } |
            ExprKind::Continue { .. } |
            ExprKind::Return { .. } |
            ExprKind::Literal { .. } |
            ExprKind::InlineAsm { .. } |
            ExprKind::Yield { .. } |
            ExprKind::Call { .. } => {
                // these are not lvalues, so we need to make a temporary.
                debug_assert!(match Category::of(&expr.kind) {
                    Some(Category::Lvalue) => false,
                    _ => true,
                });
                let temp = unpack!(block = this.as_temp(block, expr.temp_lifetime, expr));
                block.and(Lvalue::Local(temp))
            }
        }
    }
}
