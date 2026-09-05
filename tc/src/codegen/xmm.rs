use crate::codegen::{self, CodeGen, instr_name};

fn is_xmm_reg(reg: iced_x86::Register) -> bool {
    use iced_x86::Register::*;
    matches!(reg, XMM0 | XMM1 | XMM2 | XMM3 | XMM4 | XMM5 | XMM6 | XMM7)
}

fn xmm_reg(reg: iced_x86::Register) -> String {
    use iced_x86::Register::*;
    match reg {
        XMM0 => "ctx.cpu.xmm.xmm0".into(),
        XMM1 => "ctx.cpu.xmm.xmm1".into(),
        XMM2 => "ctx.cpu.xmm.xmm2".into(),
        XMM3 => "ctx.cpu.xmm.xmm3".into(),
        XMM4 => "ctx.cpu.xmm.xmm4".into(),
        XMM5 => "ctx.cpu.xmm.xmm5".into(),
        XMM6 => "ctx.cpu.xmm.xmm6".into(),
        XMM7 => "ctx.cpu.xmm.xmm7".into(),
        _ => unreachable!(),
    }
}

impl<'a> CodeGen<'a> {
    fn xmm_get(&self, instr: &iced_x86::Instruction, n: u32) -> String {
        use iced_x86::OpKind::*;
        match instr.op_kind(n) {
            Register => {
                let reg = instr.op_register(n);
                if is_xmm_reg(reg) {
                    xmm_reg(reg)
                } else {
                    self.get_op(instr, n)
                }
            }
            Memory => {
                let addr = self.gen_addr(instr);
                codegen::get_mem("[u32; 4]".into(), addr)
            }
            k => todo!("{k:?}"),
        }
    }

    fn xmm_get_64(&self, instr: &iced_x86::Instruction, n: u32) -> String {
        use iced_x86::OpKind::*;
        match instr.op_kind(n) {
            Register => format!("low_qword({})", xmm_reg(instr.op_register(n))),
            Memory => {
                let addr = self.gen_addr(instr);
                codegen::get_mem("[u32; 2]".into(), addr)
            }
            k => todo!("{k:?}"),
        }
    }

    fn xmm_get_32(&self, instr: &iced_x86::Instruction, n: u32) -> String {
        use iced_x86::OpKind::*;
        match instr.op_kind(n) {
            Register => {
                let reg = instr.op_register(n);
                if is_xmm_reg(reg) {
                    format!("{}[0]", xmm_reg(reg))
                } else {
                    self.get_op(instr, n)
                }
            }
            Memory => {
                let addr = self.gen_addr(instr);
                codegen::get_mem("u32".into(), addr)
            }
            k => todo!("{k:?}"),
        }
    }

    fn xmm_set(&self, instr: &iced_x86::Instruction, n: u32, expr: String) -> String {
        use iced_x86::OpKind::*;
        match instr.op_kind(n) {
            Register => {
                let reg = instr.op_register(n);
                if is_xmm_reg(reg) {
                    format!("{} = {};", xmm_reg(reg), expr)
                } else {
                    self.set_op(instr, n, expr)
                }
            }
            Memory => {
                let addr = self.gen_addr(instr);
                codegen::set_mem("[u32; 4]".into(), addr, expr)
            }
            _ => unreachable!(),
        }
    }

    pub fn codegen_xmm(&mut self, instr: &iced_x86::Instruction) -> bool {
        use iced_x86::Mnemonic::*;
        match instr.mnemonic() {
            // Aligned and unaligned 128-bit moves are identical in the emulated
            // flat memory model; both copy 16 bytes without any alignment check.
            Movups | Movaps => self.line(self.xmm_set(instr, 0, self.xmm_get(instr, 1))),

            // Packed single-precision arithmetic and bitwise operations.
            Addps | Subps | Mulps | Divps | Andps | Andnps | Orps | Xorps => {
                let func = instr_name(instr);
                self.line(self.xmm_set(
                    instr,
                    0,
                    format!(
                        "{func}({}, {})",
                        self.xmm_get(instr, 0),
                        self.xmm_get(instr, 1)
                    ),
                ));
            }

            // Packed single-precision unary math. RSQRTPS and RCPPS are
            // approximations on real hardware; the emulated host computes the
            // full-precision values.
            Sqrtps | Rsqrtps | Rcpps => {
                let func = instr_name(instr);
                self.line(self.xmm_set(instr, 0, format!("{func}({})", self.xmm_get(instr, 1))));
            }

            // Packed single-precision comparisons. MINPS/MAXPS preserve the
            // non-NaN operand when one is NaN; CMPPS uses the 3-bit predicate
            // in the trailing immediate.
            Minps | Maxps => {
                let func = instr_name(instr);
                self.line(self.xmm_set(
                    instr,
                    0,
                    format!(
                        "{func}({}, {})",
                        self.xmm_get(instr, 0),
                        self.xmm_get(instr, 1)
                    ),
                ));
            }
            Cmpps => {
                let pred = format!("{:#x}", instr.immediate8());
                self.line(self.xmm_set(
                    instr,
                    0,
                    format!(
                        "cmpps({}, {}, {pred})",
                        self.xmm_get(instr, 0),
                        self.xmm_get(instr, 1)
                    ),
                ));
            }

            // Packed single-precision lane shuffles.
            Shufps => {
                let imm = format!("{:#x}", instr.immediate8());
                self.line(self.xmm_set(
                    instr,
                    0,
                    format!(
                        "shufps({}, {}, {imm})",
                        self.xmm_get(instr, 0),
                        self.xmm_get(instr, 1)
                    ),
                ));
            }
            Unpckhps | Unpcklps => {
                let func = instr_name(instr);
                self.line(self.xmm_set(
                    instr,
                    0,
                    format!(
                        "{func}({}, {})",
                        self.xmm_get(instr, 0),
                        self.xmm_get(instr, 1)
                    ),
                ));
            }

            // Scalar and cross-lane partial-width moves. MOVSS only touches the
            // low 32 bits; MOVHLPS/MOVLHPS move 64 bits between high/low qwords.
            Movss => {
                let src = self.xmm_get_32(instr, 1);
                use iced_x86::OpKind::*;
                match instr.op_kind(0) {
                    Register => {
                        let dst = self.xmm_get(instr, 0);
                        let reg = instr.op_register(0);
                        self.line(format!("{} = movss({}, {});", xmm_reg(reg), dst, src));
                    }
                    Memory => {
                        let addr = self.gen_addr(instr);
                        self.line(codegen::set_mem("u32".into(), addr, src));
                    }
                    _ => unreachable!(),
                }
            }
            Movhlps | Movlhps => {
                let func = instr_name(instr);
                self.line(self.xmm_set(
                    instr,
                    0,
                    format!(
                        "{func}({}, {})",
                        self.xmm_get(instr, 0),
                        self.xmm_get(instr, 1)
                    ),
                ));
            }

            // Scalar single-precision arithmetic. Only the low 32-bit lane is
            // modified; the three high lanes are preserved.
            Addss | Subss | Mulss | Divss | Minss | Maxss => {
                let func = instr_name(instr);
                let src = self.xmm_get_32(instr, 1);
                let dst = self.xmm_get(instr, 0);
                let reg = instr.op_register(0);
                self.line(format!("{} = {func}({}, {});", xmm_reg(reg), dst, src));
            }

            // Scalar single-precision unary math.
            Sqrtss | Rsqrtss | Rcpss => {
                let func = instr_name(instr);
                let src = self.xmm_get_32(instr, 1);
                let dst = self.xmm_get(instr, 0);
                let reg = instr.op_register(0);
                self.line(format!("{} = {func}({}, {});", xmm_reg(reg), dst, src));
            }

            // Scalar single-precision comparison.
            Cmpss => {
                let pred = format!("{:#x}", instr.immediate8());
                let src = self.xmm_get_32(instr, 1);
                let dst = self.xmm_get(instr, 0);
                let reg = instr.op_register(0);
                self.line(format!(
                    "{} = cmpss({}, {}, {pred});",
                    xmm_reg(reg),
                    dst,
                    src
                ));
            }

            // Scalar int32/float conversions. CVTSI2SS converts a 32-bit signed
            // int to a float in the low lane; CVTSS2SI/CVTTSS2SI convert the low
            // float to a 32-bit signed int (round-to-nearest vs. truncate).
            Cvtsi2ss => {
                let src = self.get_op(instr, 1);
                let dst = self.xmm_get(instr, 0);
                let reg = instr.op_register(0);
                self.line(format!("{} = cvtsi2ss({}, {});", xmm_reg(reg), dst, src));
            }
            Cvtss2si | Cvttss2si => {
                let func = instr_name(instr);
                let src = self.xmm_get_32(instr, 1);
                self.line(self.set_op(instr, 0, format!("{func}({src})")));
            }

            // MMX/XMM packed conversions. CVTPI2PS reads a 64-bit MMX value and
            // converts two int32s to two floats in the low qword of the XMM
            // destination. CVTPS2PI/CVTTPS2PI read the low qword of an XMM
            // source (or a 64-bit memory pair) and convert two floats to two
            // int32s in the MMX destination.
            Cvtpi2ps => {
                let src = self.mmx_get(instr, 1);
                let dst = self.xmm_get(instr, 0);
                self.line(self.xmm_set(instr, 0, format!("cvtpi2ps({}, {})", dst, src)));
            }
            Cvtps2pi | Cvttps2pi => {
                let func = instr_name(instr);
                let src = self.xmm_get_64(instr, 1);
                self.line(self.mmx_set(instr, 0, format!("{func}({src})")));
            }

            // Scalar ordered/unordered compare that updates EFLAGS. The helper
            // preserves DF/IF/etc while setting/clearing CF/ZF/PF/OF/SF/AF.
            Comiss | Ucomiss => {
                let func = instr_name(instr);
                let a = self.xmm_get_32(instr, 0);
                let b = self.xmm_get_32(instr, 1);
                self.line(format!(
                    "ctx.cpu.flags = {func}_update_flags(ctx.cpu.flags, {a}, {b});"
                ));
            }

            // Extract the top bit of each packed float lane into a 4-bit mask
            // in a GPR.
            Movmskps => {
                let src = self.xmm_get(instr, 1);
                self.line(self.set_op(instr, 0, format!("movmskps({src})")));
            }

            // 64-bit low/high loads and stores. Memory loads replace the
            // corresponding qword and leave the other qword unchanged; stores
            // write the selected qword from a register source.
            Movlps | Movhps => {
                use iced_x86::OpKind::*;
                let func = instr_name(instr);
                match (instr.op_kind(0), instr.op_kind(1)) {
                    (Register, Memory) => {
                        let src = codegen::get_mem("[u32; 2]".into(), self.gen_addr(instr));
                        let dst = self.xmm_get(instr, 0);
                        let reg = instr.op_register(0);
                        self.line(format!("{} = {func}({}, {});", xmm_reg(reg), dst, src));
                    }
                    (Memory, Register) => {
                        let qword = if func == "movlps" {
                            "low_qword"
                        } else {
                            "high_qword"
                        };
                        let addr = self.gen_addr(instr);
                        let src = self.xmm_get(instr, 1);
                        self.line(codegen::set_mem(
                            "[u32; 2]".into(),
                            addr,
                            format!("{qword}({src})"),
                        ));
                    }
                    _ => unreachable!(),
                }
            }

            _ => return false,
        }
        true
    }
}
