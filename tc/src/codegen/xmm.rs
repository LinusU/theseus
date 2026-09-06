use crate::codegen::{self, CodeGen, instr_name};

pub(crate) fn is_xmm_reg(reg: iced_x86::Register) -> bool {
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
            k if codegen::is_memory_op(k) => {
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
            k if codegen::is_memory_op(k) => {
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
            k if codegen::is_memory_op(k) => {
                let addr = self.gen_addr(instr);
                codegen::get_mem("u32".into(), addr)
            }
            k => todo!("{k:?}"),
        }
    }

    // SSE2 packed shift instructions take the shift count as an immediate,
    // another XMM/MMX register's low qword, or a 64-bit memory location.
    fn xmm_get_shift_count(&self, instr: &iced_x86::Instruction, n: u32) -> String {
        use iced_x86::OpKind::*;
        match instr.op_kind(n) {
            Immediate8 => format!("{:#x}u64", instr.immediate8()),
            Register => format!("low_qword({})", xmm_reg(instr.op_register(n))),
            k if codegen::is_memory_op(k) => {
                let addr = self.gen_addr(instr);
                codegen::get_mem("[u32; 2]".into(), addr)
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
            k if codegen::is_memory_op(k) => {
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
            // MOVNTPS/MOVNTPD/MOVNTDQ are also pure stores in this model.
            Movups | Movaps | Movntps | Movupd | Movapd | Movntpd | Movdqu | Movdqa | Movntdq => {
                self.line(self.xmm_set(instr, 0, self.xmm_get(instr, 1)))
            }

            // 32-bit/64-bit scalar moves between GPRs, memory, and XMM registers.
            // MOVD clears the upper 96 bits of the destination; MOVQ clears the
            // upper 64 bits.
            Movd => {
                if is_xmm_reg(instr.op_register(0)) {
                    let src = self.get_op(instr, 1);
                    self.line(self.xmm_set(instr, 0, format!("movd_to_xmm({src})")));
                } else {
                    self.line(self.set_op(instr, 0, self.xmm_get_32(instr, 1)));
                }
            }
            Movq => {
                if is_xmm_reg(instr.op_register(0)) {
                    let src = self.xmm_get_64(instr, 1);
                    self.line(self.xmm_set(instr, 0, format!("movq_to_xmm({src})")));
                } else {
                    let src = self.xmm_get(instr, 1);
                    self.line(codegen::set_mem(
                        "u64".into(),
                        self.gen_addr(instr),
                        format!("movq_from_xmm({src})"),
                    ));
                }
            }
            // MOVQ2DQ zero-extends the low qword of an MMX register into an
            // XMM register.
            Movq2dq => {
                let src = self.mmx_get(instr, 1);
                self.line(self.xmm_set(instr, 0, format!("movq2dq({src})")));
            }

            // MASKMOVDQU stores the source to the implicit DS:(E)DI destination,
            // writing a byte only when the corresponding mask byte's high bit is
            // set. The mask is always in XMM0.
            Maskmovdqu => {
                let src = self.xmm_get(instr, 2);
                let mask = self.xmm_get(instr, 1);
                self.line(format!("ctx.maskmovdqu({src}, {mask});"));
            }

            // Packed single/double-precision arithmetic and bitwise operations.
            Addps | Subps | Mulps | Divps | Andps | Andnps | Orps | Xorps | Addpd | Subpd
            | Mulpd | Divpd | Andpd | Andnpd | Orpd | Xorpd => {
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

            // Packed single/double-precision comparisons. MINPS/MAXPS return
            // the second source operand when either operand is NaN or the
            // values tie; CMPPS/CMPPD use the 3-bit predicate in the trailing
            // immediate.
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
            Cmppd => {
                let pred = format!("{:#x}", instr.immediate8());
                self.line(self.xmm_set(
                    instr,
                    0,
                    format!(
                        "cmppd({}, {}, {pred})",
                        self.xmm_get(instr, 0),
                        self.xmm_get(instr, 1)
                    ),
                ));
            }
            Cmpsd => {
                let pred = format!("{:#x}", instr.immediate8());
                let src = self.xmm_get_64(instr, 1);
                let dst = self.xmm_get(instr, 0);
                let reg = instr.op_register(0);
                self.line(format!(
                    "{} = cmpsd({}, {}, {pred});",
                    xmm_reg(reg),
                    dst,
                    src
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

            // Scalar and cross-lane partial-width moves. MOVSS/MOVNTSS only
            // touch the low 32 bits; MOVHLPS/MOVLHPS move 64 bits between
            // high/low qwords. MOVSD replaces the low 64 bits.
            Movss | Movntss => {
                let src = self.xmm_get_32(instr, 1);
                let kind = instr.op_kind(0);
                if kind == iced_x86::OpKind::Register {
                    let dst = self.xmm_get(instr, 0);
                    let reg = instr.op_register(0);
                    self.line(format!("{} = movss({}, {});", xmm_reg(reg), dst, src));
                } else if codegen::is_memory_op(kind) {
                    let addr = self.gen_addr(instr);
                    self.line(codegen::set_mem("u32".into(), addr, src));
                } else {
                    unreachable!()
                }
            }
            Movsd => {
                let src = self.xmm_get_64(instr, 1);
                let kind = instr.op_kind(0);
                if kind == iced_x86::OpKind::Register {
                    let dst = self.xmm_get(instr, 0);
                    let reg = instr.op_register(0);
                    self.line(format!("{} = movsd({}, {});", xmm_reg(reg), dst, src));
                } else if codegen::is_memory_op(kind) {
                    let addr = self.gen_addr(instr);
                    self.line(codegen::set_mem("[u32; 2]".into(), addr, src));
                } else {
                    unreachable!()
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

            // Scalar double-precision arithmetic. Only the low 64-bit qword is
            // modified; the high qword is preserved.
            Addsd | Subsd | Mulsd | Divsd => {
                let func = instr_name(instr);
                let src = self.xmm_get_64(instr, 1);
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

            // Scalar int64/float64 conversions. CVTSI2SD converts a 32-bit signed
            // int to a double in the low qword; CVTSD2SI/CVTTSD2SI convert the low
            // double to a 32-bit signed int (round-to-nearest vs. truncate).
            Cvtsi2sd => {
                let src = self.get_op(instr, 1);
                let dst = self.xmm_get(instr, 0);
                let reg = instr.op_register(0);
                self.line(format!("{} = cvtsi2sd({}, {});", xmm_reg(reg), dst, src));
            }
            Cvtsd2si | Cvttsd2si => {
                let func = instr_name(instr);
                let src = self.xmm_get_64(instr, 1);
                self.line(self.set_op(instr, 0, format!("{func}({src})")));
            }

            // Scalar mixed-precision and packed double/single conversions.
            Cvtsd2ss => {
                let src = self.xmm_get_64(instr, 1);
                let dst = self.xmm_get(instr, 0);
                let reg = instr.op_register(0);
                self.line(format!("{} = cvtsd2ss({}, {});", xmm_reg(reg), dst, src));
            }
            Cvtss2sd => {
                let src = self.xmm_get_32(instr, 1);
                let dst = self.xmm_get(instr, 0);
                let reg = instr.op_register(0);
                self.line(format!("{} = cvtss2sd({}, {});", xmm_reg(reg), dst, src));
            }
            Cvtpd2ps | Cvtps2pd | Cvtdq2ps | Cvtps2dq | Cvttps2dq | Cvtdq2pd | Cvtpd2dq
            | Cvttpd2dq => {
                let func = instr_name(instr);
                let src = self.xmm_get(instr, 1);
                self.line(self.xmm_set(instr, 0, format!("{func}({src})")));
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

            // Packed integer shifts. PSLLDQ/PSRLDQ are byte shifts of the whole
            // 128-bit value; PSLLW/PSLLD/PSLLQ and PSRLW/PSRLD/PSRLQ are logical
            // lane shifts; PSRAW/PSRAD are arithmetic lane shifts.
            Pslldq | Psrldq => {
                let func = instr_name(instr);
                let count = format!("{:#x}u64", instr.immediate8());
                let dst = self.xmm_get(instr, 0);
                self.line(self.xmm_set(instr, 0, format!("{func}({dst}, {count})")));
            }
            Psllw | Pslld | Psllq | Psrlw | Psrld | Psrlq | Psraw | Psrad => {
                // These mnemonics are also used by the MMX integer shifters,
                // so the 128-bit XMM variants use a distinct helper name.
                let func = format!("{}_xmm", instr_name(instr));
                let count = self.xmm_get_shift_count(instr, 1);
                let dst = self.xmm_get(instr, 0);
                self.line(self.xmm_set(instr, 0, format!("{func}({dst}, {count})")));
            }

            // Packed unpack and shuffle. PUNPCK* interleave low/high lanes from
            // two 128-bit sources; PSHUFD/LW/HW permute lanes using the 8-bit
            // immediate. The MMX versions of PUNPCK* use the same mnemonics, so
            // the 128-bit variants are emitted with an _xmm suffix.
            Punpcklbw | Punpcklwd | Punpckldq | Punpckhbw | Punpckhwd | Punpckhdq | Punpcklqdq
            | Punpckhqdq => {
                let func = format!("{}_xmm", instr_name(instr));
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
            Pshufd => {
                let imm = format!("{:#x}", instr.immediate8());
                let src = self.xmm_get(instr, 1);
                self.line(self.xmm_set(instr, 0, format!("pshufd_xmm({src}, {imm})")));
            }
            // PSHUFB uses the destination as the source bytes and the second
            // operand as the 128-bit control mask.
            Pshufb => {
                self.line(self.xmm_set(
                    instr,
                    0,
                    format!(
                        "pshufb_xmm({}, {})",
                        self.xmm_get(instr, 0),
                        self.xmm_get(instr, 1)
                    ),
                ));
            }
            // PABSB/W/D (SSSE3) are unary absolute-value operations.
            Pabsb | Pabsw | Pabsd => {
                let func = format!("{}_xmm", instr_name(instr));
                let src = self.xmm_get(instr, 1);
                self.line(self.xmm_set(instr, 0, format!("{func}({src})")));
            }
            Pshuflw | Pshufhw => {
                let func = format!("{}_xmm", instr_name(instr));
                let imm = format!("{:#x}", instr.immediate8());
                let src = self.xmm_get(instr, 1);
                self.line(self.xmm_set(instr, 0, format!("{func}({src}, {imm})")));
            }

            // Packed integer add/sub and bitwise logic. These mnemonics are also
            // used by MMX, so the 128-bit XMM variants use an _xmm suffix.
            Paddb | Paddw | Paddd | Paddq | Psubb | Psubw | Psubd | Psubq | Pand | Pandn | Por
            | Pxor | Phaddw | Phaddd | Phaddsw | Phsubw | Phsubd | Phsubsw | Pmaddubsw
            | Pmulhrsw | Psignb | Psignw | Psignd => {
                let func = format!("{}_xmm", instr_name(instr));
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

            // Packed integer compare, average, and min/max. These mnemonics are
            // shared with MMX, so the 128-bit XMM variants use an _xmm suffix.
            Pcmpeqb | Pcmpeqw | Pcmpeqd | Pcmpgtb | Pcmpgtw | Pcmpgtd | Pavgb | Pavgw | Pmaxub
            | Pminub | Pmaxsw | Pminsw => {
                let func = format!("{}_xmm", instr_name(instr));
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

            // Packed integer saturating arithmetic and pack. These mnemonics are
            // shared with MMX, so the 128-bit XMM variants use an _xmm suffix.
            Paddsb | Paddsw | Paddusb | Paddusw | Psubsb | Psubsw | Psubusb | Psubusw
            | Packsswb | Packssdw | Packuswb => {
                let func = format!("{}_xmm", instr_name(instr));
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
            Comisd | Ucomisd => {
                let func = instr_name(instr);
                let a = self.xmm_get_64(instr, 0);
                let b = self.xmm_get_64(instr, 1);
                self.line(format!(
                    "ctx.cpu.flags = {func}_update_flags(ctx.cpu.flags, {a}, {b});"
                ));
            }

            // Extract the top bit of each packed float/double lane into a 4/2-bit
            // mask in a GPR.
            Movmskps => {
                let src = self.xmm_get(instr, 1);
                self.line(self.set_op(instr, 0, format!("movmskps({src})")));
            }
            Movmskpd => {
                let src = self.xmm_get(instr, 1);
                self.line(self.set_op(instr, 0, format!("movmskpd({src})")));
            }

            // Extract a 16-bit mask of the sign bits of each byte, or extract
            // or insert a single word lane in a 128-bit XMM register.
            Pmovmskb => {
                let src = self.xmm_get(instr, 1);
                self.line(self.set_op(instr, 0, format!("pmovmskb_xmm({src})")));
            }
            Pextrw => {
                let src = self.xmm_get(instr, 1);
                let sel = self.get_op(instr, 2);
                let expr = format!("pextrw_xmm({src}, {sel})");
                if codegen::is_memory_op(instr.op_kind(0)) {
                    self.line(self.set_op(instr, 0, format!("({expr} as u16)")));
                } else {
                    self.line(self.set_op(instr, 0, expr));
                }
            }
            Pinsrw => {
                let src = self.get_op(instr, 1);
                let sel = self.get_op(instr, 2);
                self.line(self.xmm_set(
                    instr,
                    0,
                    format!(
                        "pinsrw_xmm({}, {src} as u16, {sel})",
                        self.xmm_get(instr, 0)
                    ),
                ));
            }

            // Packed multiply and absolute-difference reductions. PSADBW sums
            // byte-lane absolute differences into the low word of each qword;
            // PMULLW keeps the low word of each product; PMADDWD adds adjacent
            // signed 16-bit products into 32-bit dwords; PMULHW/PMULHUW keep
            // the high word of signed/unsigned word products; PMULUDQ multiplies
            // the low dword of each qword.
            Psadbw | Pmullw | Pmaddwd | Pmulhw | Pmulhuw | Pmuludq => {
                let func = format!("{}_xmm", instr_name(instr));
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

            // 64-bit low/high loads and stores. Memory loads replace the
            // corresponding qword and leave the other qword unchanged; stores
            // write the selected qword from a register source.
            Movlps | Movhps => {
                use iced_x86::OpKind::*;
                let func = instr_name(instr);
                let (op0, op1) = (instr.op_kind(0), instr.op_kind(1));
                if op0 == Register && codegen::is_memory_op(op1) {
                    let src = codegen::get_mem("[u32; 2]".into(), self.gen_addr(instr));
                    let dst = self.xmm_get(instr, 0);
                    let reg = instr.op_register(0);
                    self.line(format!("{} = {func}({}, {});", xmm_reg(reg), dst, src));
                } else if codegen::is_memory_op(op0) && op1 == Register {
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
                } else {
                    unreachable!()
                }
            }

            Palignr => {
                let imm = format!("{:#x}", instr.immediate8());
                let func = format!("{}_xmm", instr_name(instr));
                self.line(self.xmm_set(
                    instr,
                    0,
                    format!(
                        "{func}({}, {}, {})",
                        self.xmm_get(instr, 0),
                        self.xmm_get(instr, 1),
                        imm
                    ),
                ));
            }

            _ => return false,
        }
        true
    }
}
