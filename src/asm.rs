use gccjit::{RValue, ToRValue, Type};
use rustc_ast::ast::{InlineAsmOptions, InlineAsmTemplatePiece};
use rustc_codegen_ssa::mir::operand::OperandValue;
use rustc_codegen_ssa::mir::place::PlaceRef;
use rustc_codegen_ssa::traits::{AsmBuilderMethods, AsmMethods, BaseTypeMethods, BuilderMethods, InlineAsmOperandRef, MiscMethods};
use rustc_data_structures::fx::FxHashMap;
use rustc_hir::{GlobalAsm, LlvmInlineAsmInner};
use rustc_middle::bug;
use rustc_middle::ty::layout::TyAndLayout;
use rustc_span::Span;
use rustc_target::asm::*;

use crate::builder::Builder;
use crate::context::CodegenCx;
use crate::type_of::LayoutGccExt;

impl<'a, 'gcc, 'tcx> AsmBuilderMethods<'tcx> for Builder<'a, 'gcc, 'tcx> {
    fn codegen_llvm_inline_asm(&mut self, _ia: &LlvmInlineAsmInner, _outputs: Vec<PlaceRef<'tcx, RValue<'gcc>>>, mut _inputs: Vec<RValue<'gcc>>, _span: Span) -> bool {
        // TODO
        return true;

        /*let mut ext_constraints = vec![];
        let mut output_types = vec![];

        // Prepare the output operands
        let mut indirect_outputs = vec![];
        for (i, (out, &place)) in ia.outputs.iter().zip(&outputs).enumerate() {
            if out.is_rw {
                let operand = self.load_operand(place);
                if let OperandValue::Immediate(_) = operand.val {
                    inputs.push(operand.immediate());
                }
                ext_constraints.push(i.to_string());
            }
            if out.is_indirect {
                let operand = self.load_operand(place);
                if let OperandValue::Immediate(_) = operand.val {
                    indirect_outputs.push(operand.immediate());
                }
            } else {
                output_types.push(place.layout.gcc_type(self.cx()));
            }
        }
        if !indirect_outputs.is_empty() {
            indirect_outputs.extend_from_slice(&inputs);
            inputs = indirect_outputs;
        }

        let clobbers = ia.clobbers.iter().map(|s| format!("~{{{}}}", &s));

        // Default per-arch clobbers
        // Basically what clang does
        let arch_clobbers = match &self.sess().target.target.arch[..] {
            "x86" | "x86_64" => vec!["~{dirflag}", "~{fpsr}", "~{flags}"],
            "mips" | "mips64" => vec!["~{$1}"],
            _ => Vec::new(),
        };

        let all_constraints = ia
            .outputs
            .iter()
            .map(|out| out.constraint.to_string())
            .chain(ia.inputs.iter().map(|s| s.to_string()))
            .chain(ext_constraints)
            .chain(clobbers)
            .chain(arch_clobbers.iter().map(|s| (*s).to_string()))
            .collect::<Vec<String>>()
            .join(",");

        debug!("Asm Constraints: {}", &all_constraints);

        // Depending on how many outputs we have, the return type is different
        let num_outputs = output_types.len();
        let output_type = match num_outputs {
            0 => self.type_void(),
            1 => output_types[0],
            _ => self.type_struct(&output_types, false),
        };

        let asm = ia.asm.as_str();
        let r = inline_asm_call(
            self,
            &asm,
            &all_constraints,
            &inputs,
            output_type,
            ia.volatile,
            ia.alignstack,
            ia.dialect,
        );
        if r.is_none() {
            return false;
        }
        let r = r.unwrap();

        // Again, based on how many outputs we have
        let outputs = ia.outputs.iter().zip(&outputs).filter(|&(ref o, _)| !o.is_indirect);
        for (i, (_, &place)) in outputs.enumerate() {
            let v = if num_outputs == 1 { r } else { self.extract_value(r, i as u64) };
            OperandValue::Immediate(v).store(self, place);
        }

        // Store mark in a metadata node so we can map LLVM errors
        // back to source locations.  See #17552.
        unsafe {
            let key = "srcloc";
            let kind = llvm::LLVMGetMDKindIDInContext(
                self.llcx,
                key.as_ptr() as *const c_char,
                key.len() as c_uint,
            );

            let val: &'ll Value = self.const_i32(span.ctxt().outer_expn().as_u32() as i32);

            llvm::LLVMSetMetadata(r, kind, llvm::LLVMMDNodeInContext(self.llcx, &val, 1));
        }

        true*/
    }

    fn codegen_inline_asm(&mut self, template: &[InlineAsmTemplatePiece], operands: &[InlineAsmOperandRef<'tcx, Self>], options: InlineAsmOptions, span: &[Span]) {
        let asm_arch = self.tcx.sess.asm_arch.unwrap();

        let intel_dialect =
            match asm_arch {
                InlineAsmArch::X86 | InlineAsmArch::X86_64 if !options.contains(InlineAsmOptions::ATT_SYNTAX) => true,
                _ => false,
            };

        // Collect the types of output operands
        // FIXME: we do this here instead of later because of a bug in libgccjit where creating the
        // variable after the extended asm expression causes a segfault:
        // https://gcc.gnu.org/bugzilla/show_bug.cgi?id=100380
        let mut output_vars = FxHashMap::default();
        let mut operand_numbers = FxHashMap::default();
        let mut current_number = 0;
        for (idx, op) in operands.iter().enumerate() {
            match *op {
                InlineAsmOperandRef::Out { reg, late, place } => {
                    let ty =
                        match place {
                            Some(place) => place.layout.gcc_type(self.cx, false),
                            None => {
                                // If the output is discarded, we don't really care what
                                // type is used. We're just using this to tell GCC to
                                // reserve the register.
                                dummy_output_type(self.cx, reg.reg_class())
                            },
                        };
                    let var = self.current_func().new_local(None, ty, "output_register");
                    operand_numbers.insert(idx, current_number);
                    current_number += 1;
                    output_vars.insert(idx, var);
                }
                InlineAsmOperandRef::InOut { reg, late, in_value, out_place } => {
                    // TODO
                }
                _ => {}
            }
        }

        // All output operands must come before the input operands, hence the 2 loops.
        for (idx, op) in operands.iter().enumerate() {
            match *op {
                InlineAsmOperandRef::In { .. } => {
                    operand_numbers.insert(idx, current_number);
                    current_number += 1;
                },
                _ => (),
            }
        }

        // Build the template string
        let mut template_str = String::new();
        let mut operand_index = 0;
        for piece in template {
            match *piece {
                InlineAsmTemplatePiece::String(ref s) => {
                    template_str.push_str(s)
                }
                InlineAsmTemplatePiece::Placeholder { operand_idx, modifier, span: _ } => {
                    match operands[operand_idx] {
                        InlineAsmOperandRef::Out { reg, place: Some(place), .. } => {
                            /*let modifier = modifier_to_llvm(asm_arch, reg.reg_class(), modifier); // TODO
                            if let Some(modifier) = modifier {
                                template_str.push_str(&format!(
                                    "${{{}:{}}}",
                                    op_idx[&operand_idx], modifier
                                ));
                            } else {*/
                                template_str.push_str(&format!("%{}", operand_numbers[&operand_idx]));
                            //}
                            //extended_asm.add_output_operand(None, "=r", place);
                        },
                        InlineAsmOperandRef::Out { reg, place: None, .. } => {
                            unimplemented!("Out None");
                            //extended_asm.add_output_operand(None, "=r", place);
                        },
                        InlineAsmOperandRef::In { reg, .. }
                        | InlineAsmOperandRef::InOut { reg, .. } => {
                            /*let modifier = modifier_to_llvm(asm_arch, reg.reg_class(), modifier);
                            if let Some(modifier) = modifier {
                                template_str.push_str(&format!(
                                    "${{{}:{}}}",
                                    op_idx[&operand_idx], modifier
                                ));
                            } else {*/
                                template_str.push_str(&format!("%{}", operand_numbers[&operand_idx]));
                            //}
                        }
                        InlineAsmOperandRef::Const { ref string } => {
                            // Const operands get injected directly into the template
                            template_str.push_str(string);
                        }
                        InlineAsmOperandRef::SymFn { .. }
                        | InlineAsmOperandRef::SymStatic { .. } => {
                            unimplemented!();
                            // Only emit the raw symbol name
                            //template_str.push_str(&format!("${{{}:c}}", op_idx[&operand_idx]));
                        }
                    }
                }
            }
        }

        let block = self.llbb();
        let template_str =
            if intel_dialect {
                // FIXME: this might break the "m" memory constraint:
                // https://stackoverflow.com/a/9347957/389119
                format!(".intel_syntax noprefix\n\t{}\n\t.att_syntax noprefix", template_str)
            }
            else {
                template_str
            };
        let extended_asm = block.add_extended_asm(None, &template_str);

        // Collect the types of output operands
        let mut output_types = vec![];
        for (idx, op) in operands.iter().enumerate() {
            match *op {
                InlineAsmOperandRef::Out { reg, late, place } => {
                    let ty =
                        match place {
                            Some(place) => place.layout.gcc_type(self.cx, false),
                            None => {
                                // If the output is discarded, we don't really care what
                                // type is used. We're just using this to tell GCC to
                                // reserve the register.
                                dummy_output_type(self.cx, reg.reg_class())
                            },
                        };
                    output_types.push(ty);
                    //op_idx.insert(idx, constraints.len());
                    let prefix = if late { "=" } else { "=&" };
                    let constraint = format!("{}{}", prefix, reg_to_gcc(reg));

                    let var = output_vars[&idx];
                    extended_asm.add_output_operand(None, &constraint, var);
                }
                InlineAsmOperandRef::InOut { reg, late, in_value, out_place } => {
                    let ty =
                        match out_place {
                            Some(out_place) => out_place.layout.gcc_type(self.cx, false),
                            None => dummy_output_type(self.cx, reg.reg_class())
                        };
                    output_types.push(ty);
                    //op_idx.insert(idx, constraints.len());
                    let prefix = if late { "=" } else { "=&" };
                    //constraints.push(format!("{}{}", prefix, reg_to_gcc(reg)));
                }
                InlineAsmOperandRef::In { reg, value } => {
                    let constraint = reg_to_gcc(reg);
                    extended_asm.add_input_operand(None, &constraint, value.immediate());
                }
                _ => {}
            }
        }

        // Collect input operands
        //let mut constraints = vec![];
        //let mut op_idx = FxHashMap::default();
        //let mut inputs = vec![];
        for (idx, op) in operands.iter().enumerate() {
            match *op {
                InlineAsmOperandRef::In { reg, value } => {
                    //inputs.push(value.immediate());
                    //op_idx.insert(idx, constraints.len());
                    //constraints.push(reg_to_gcc(reg));
                }
                InlineAsmOperandRef::InOut { reg, late: _, in_value, out_place: _ } => {
                    //inputs.push(in_value.immediate());
                    //constraints.push(format!("{}", op_idx[&idx]));
                }
                InlineAsmOperandRef::SymFn { instance } => {
                    //inputs.push(self.cx.get_fn(instance));
                    //op_idx.insert(idx, constraints.len());
                    //constraints.push("s".to_string());
                }
                InlineAsmOperandRef::SymStatic { def_id } => {
                    //inputs.push(self.cx.get_static(def_id));
                    //op_idx.insert(idx, constraints.len());
                    //constraints.push("s".to_string());
                }
                _ => {}
            }
        }

        /*if !options.contains(InlineAsmOptions::PRESERVES_FLAGS) {
            match asm_arch {
                InlineAsmArch::AArch64 | InlineAsmArch::Arm => {
                    constraints.push("~{cc}".to_string());
                }
                InlineAsmArch::X86 | InlineAsmArch::X86_64 => {
                    constraints.extend_from_slice(&[
                        "~{dirflag}".to_string(),
                        "~{fpsr}".to_string(),
                        "~{flags}".to_string(),
                    ]);
                }
                InlineAsmArch::RiscV32 | InlineAsmArch::RiscV64 => {}
            }
        }
        if !options.contains(InlineAsmOptions::NOMEM) {
            // This is actually ignored by LLVM, but it's probably best to keep
            // it just in case. LLVM instead uses the ReadOnly/ReadNone
            // attributes on the call instruction to optimize.
            constraints.push("~{memory}".to_string());
        }
        let volatile = !options.contains(InlineAsmOptions::PURE);
        let alignstack = !options.contains(InlineAsmOptions::NOSTACK);
        let output_type = match &output_types[..] {
            [] => self.type_void(),
            [ty] => ty,
            tys => self.type_struct(&tys, false),
        };*/

        /*let result = inline_asm_call(
            self,
            &template_str,
            &constraints.join(","),
            &inputs,
            output_type,
            volatile,
            alignstack,
            dialect,
            span,
        )
        .unwrap_or_else(|| span_bug!(span, "LLVM asm constraint validation failed"));

        if options.contains(InlineAsmOptions::PURE) {
            if options.contains(InlineAsmOptions::NOMEM) {
                llvm::Attribute::ReadNone.apply_callsite(llvm::AttributePlace::Function, result);
            } else if options.contains(InlineAsmOptions::READONLY) {
                llvm::Attribute::ReadOnly.apply_callsite(llvm::AttributePlace::Function, result);
            }
        } else {
            if options.contains(InlineAsmOptions::NOMEM) {
                llvm::Attribute::InaccessibleMemOnly
                    .apply_callsite(llvm::AttributePlace::Function, result);
            } else {
                // LLVM doesn't have an attribute to represent ReadOnly + SideEffect
            }
        }*/

        // Write results to outputs
        for (idx, op) in operands.iter().enumerate() {
            if let InlineAsmOperandRef::Out { reg, place: Some(place), .. }
            | InlineAsmOperandRef::InOut { reg, out_place: Some(place), .. } = *op
            {
                OperandValue::Immediate(output_vars[&idx].to_rvalue()).store(self, place);
            }
        }
    }
}

/// Converts a register class to a GCC constraint code.
fn reg_to_gcc<'tcx>(reg: InlineAsmRegOrRegClass) -> String {
    match reg {
        // For vector registers LLVM wants the register name to match the type size.
        InlineAsmRegOrRegClass::Reg(reg) => unimplemented!(),
        InlineAsmRegOrRegClass::RegClass(reg) => match reg {
            InlineAsmRegClass::AArch64(AArch64InlineAsmRegClass::reg) => unimplemented!(),
            InlineAsmRegClass::AArch64(AArch64InlineAsmRegClass::vreg) => unimplemented!(),
            InlineAsmRegClass::AArch64(AArch64InlineAsmRegClass::vreg_low16) => unimplemented!(),
            InlineAsmRegClass::Arm(ArmInlineAsmRegClass::reg) => unimplemented!(),
            InlineAsmRegClass::Arm(ArmInlineAsmRegClass::reg_thumb) => unimplemented!(),
            InlineAsmRegClass::Arm(ArmInlineAsmRegClass::sreg)
            | InlineAsmRegClass::Arm(ArmInlineAsmRegClass::dreg_low16)
            | InlineAsmRegClass::Arm(ArmInlineAsmRegClass::qreg_low8) => unimplemented!(),
            InlineAsmRegClass::Arm(ArmInlineAsmRegClass::sreg_low16)
            | InlineAsmRegClass::Arm(ArmInlineAsmRegClass::dreg_low8)
            | InlineAsmRegClass::Arm(ArmInlineAsmRegClass::qreg_low4) => unimplemented!(),
            InlineAsmRegClass::Arm(ArmInlineAsmRegClass::dreg)
            | InlineAsmRegClass::Arm(ArmInlineAsmRegClass::qreg) => unimplemented!(),
            InlineAsmRegClass::Hexagon(HexagonInlineAsmRegClass::reg) => unimplemented!(),
            InlineAsmRegClass::Mips(MipsInlineAsmRegClass::reg) => unimplemented!(),
            InlineAsmRegClass::Mips(MipsInlineAsmRegClass::freg) => unimplemented!(),
            InlineAsmRegClass::Nvptx(NvptxInlineAsmRegClass::reg16) => unimplemented!(),
            InlineAsmRegClass::Nvptx(NvptxInlineAsmRegClass::reg32) => unimplemented!(),
            InlineAsmRegClass::Nvptx(NvptxInlineAsmRegClass::reg64) => unimplemented!(),
            InlineAsmRegClass::RiscV(RiscVInlineAsmRegClass::reg) => unimplemented!(),
            InlineAsmRegClass::RiscV(RiscVInlineAsmRegClass::freg) => unimplemented!(),
            InlineAsmRegClass::X86(X86InlineAsmRegClass::reg) => "r",
            InlineAsmRegClass::X86(X86InlineAsmRegClass::reg_abcd) => unimplemented!(),
            InlineAsmRegClass::X86(X86InlineAsmRegClass::reg_byte) => unimplemented!(),
            InlineAsmRegClass::X86(X86InlineAsmRegClass::xmm_reg)
            | InlineAsmRegClass::X86(X86InlineAsmRegClass::ymm_reg) => unimplemented!(),
            InlineAsmRegClass::X86(X86InlineAsmRegClass::zmm_reg) => unimplemented!(),
            InlineAsmRegClass::X86(X86InlineAsmRegClass::kreg) => unimplemented!(),
            InlineAsmRegClass::Wasm(WasmInlineAsmRegClass::local) => unimplemented!(),
            InlineAsmRegClass::SpirV(SpirVInlineAsmRegClass::reg) => {
                bug!("GCC backend does not support SPIR-V")
            }
        }
        .to_string(),
    }
}

/// Type to use for outputs that are discarded. It doesn't really matter what
/// the type is, as long as it is valid for the constraint code.
fn dummy_output_type<'gcc, 'tcx>(cx: &CodegenCx<'gcc, 'tcx>, reg: InlineAsmRegClass) -> Type<'gcc> {
    match reg {
        InlineAsmRegClass::AArch64(AArch64InlineAsmRegClass::reg) => cx.type_i32(),
        InlineAsmRegClass::AArch64(AArch64InlineAsmRegClass::vreg)
        | InlineAsmRegClass::AArch64(AArch64InlineAsmRegClass::vreg_low16) => {
            unimplemented!()
        }
        InlineAsmRegClass::Arm(ArmInlineAsmRegClass::reg)
        | InlineAsmRegClass::Arm(ArmInlineAsmRegClass::reg_thumb) => cx.type_i32(),
        InlineAsmRegClass::Arm(ArmInlineAsmRegClass::sreg)
        | InlineAsmRegClass::Arm(ArmInlineAsmRegClass::sreg_low16) => cx.type_f32(),
        InlineAsmRegClass::Arm(ArmInlineAsmRegClass::dreg)
        | InlineAsmRegClass::Arm(ArmInlineAsmRegClass::dreg_low16)
        | InlineAsmRegClass::Arm(ArmInlineAsmRegClass::dreg_low8) => cx.type_f64(),
        InlineAsmRegClass::Arm(ArmInlineAsmRegClass::qreg)
        | InlineAsmRegClass::Arm(ArmInlineAsmRegClass::qreg_low8)
        | InlineAsmRegClass::Arm(ArmInlineAsmRegClass::qreg_low4) => {
            unimplemented!()
        }
        InlineAsmRegClass::Hexagon(HexagonInlineAsmRegClass::reg) => cx.type_i32(),
        InlineAsmRegClass::Mips(MipsInlineAsmRegClass::reg) => cx.type_i32(),
        InlineAsmRegClass::Mips(MipsInlineAsmRegClass::freg) => cx.type_f32(),
        InlineAsmRegClass::Nvptx(NvptxInlineAsmRegClass::reg16) => cx.type_i16(),
        InlineAsmRegClass::Nvptx(NvptxInlineAsmRegClass::reg32) => cx.type_i32(),
        InlineAsmRegClass::Nvptx(NvptxInlineAsmRegClass::reg64) => cx.type_i64(),
        InlineAsmRegClass::RiscV(RiscVInlineAsmRegClass::reg) => cx.type_i32(),
        InlineAsmRegClass::RiscV(RiscVInlineAsmRegClass::freg) => cx.type_f32(),
        InlineAsmRegClass::X86(X86InlineAsmRegClass::reg)
        | InlineAsmRegClass::X86(X86InlineAsmRegClass::reg_abcd) => cx.type_i32(),
        InlineAsmRegClass::X86(X86InlineAsmRegClass::reg_byte) => cx.type_i8(),
        InlineAsmRegClass::X86(X86InlineAsmRegClass::xmm_reg)
        | InlineAsmRegClass::X86(X86InlineAsmRegClass::ymm_reg)
        | InlineAsmRegClass::X86(X86InlineAsmRegClass::zmm_reg) => cx.type_f32(),
        InlineAsmRegClass::X86(X86InlineAsmRegClass::kreg) => cx.type_i16(),
        InlineAsmRegClass::Wasm(WasmInlineAsmRegClass::local) => cx.type_i32(),
        InlineAsmRegClass::SpirV(SpirVInlineAsmRegClass::reg) => {
            bug!("LLVM backend does not support SPIR-V")
        }
    }
}

impl<'gcc, 'tcx> AsmMethods for CodegenCx<'gcc, 'tcx> {
    fn codegen_global_asm(&self, ga: &GlobalAsm) {
        let asm = ga.asm.as_str();
        // TODO
        //unsafe {
            //llvm::LLVMRustAppendModuleInlineAsm(self.llmod, asm.as_ptr().cast(), asm.len());
        //}
    }
}
