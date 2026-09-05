use crate::codegen::{self, CodeGen, instr_name};

fn is_mmx_reg(reg: iced_x86::Register) -> bool {
    use iced_x86::Register::*;
    matches!(reg, MM0 | MM1 | MM2 | MM3 | MM4 | MM5 | MM6 | MM7)
}

fn mmx_reg(reg: iced_x86::Register) -> String {
    use iced_x86::Register::*;
    match reg {
        MM0 => "ctx.cpu.mmx.mm0".into(),
        MM1 => "ctx.cpu.mmx.mm1".into(),
        MM2 => "ctx.cpu.mmx.mm2".into(),
        MM3 => "ctx.cpu.mmx.mm3".into(),
        MM4 => "ctx.cpu.mmx.mm4".into(),
        MM5 => "ctx.cpu.mmx.mm5".into(),
        MM6 => "ctx.cpu.mmx.mm6".into(),
        MM7 => "ctx.cpu.mmx.mm7".into(),
        _ => unreachable!(),
    }
}

impl<'a> CodeGen<'a> {
    pub(crate) fn mmx_get(&self, instr: &iced_x86::Instruction, n: u32) -> String {
        use iced_x86::OpKind::*;
        match instr.op_kind(n) {
            Register => mmx_reg(instr.op_register(n)),
            Memory => {
                let addr = self.gen_addr(instr);
                let size = codegen::mem_size(instr);
                codegen::get_mem(codegen::type_for_size(size), addr)
            }
            Immediate8 => format!("{:#x}u64", instr.immediate8()),
            k => todo!("{k:?}"),
        }
    }

    fn mmx_get_32(&self, instr: &iced_x86::Instruction, n: u32) -> String {
        use iced_x86::OpKind::*;
        if matches!(instr.op_kind(n), Register) {
            let reg = instr.op_register(n);
            if is_mmx_reg(reg) {
                return format!("{} as u32", mmx_reg(instr.op_register(n)));
            }
        }
        self.get_op(instr, n)
    }

    pub(crate) fn mmx_set(&self, instr: &iced_x86::Instruction, n: u32, expr: String) -> String {
        use iced_x86::OpKind::*;
        match instr.op_kind(n) {
            Register => format!("{} = {};", mmx_reg(instr.op_register(n)), expr),
            Memory => {
                let addr = self.gen_addr(instr);
                codegen::set_mem("u64".into(), addr, expr)
            }
            _ => unreachable!(),
        }
    }

    fn mmx_set_32(&self, instr: &iced_x86::Instruction, n: u32, expr: String) -> String {
        use iced_x86::OpKind::*;
        if matches!(instr.op_kind(n), Register) {
            let reg = instr.op_register(n);
            if is_mmx_reg(reg) {
                return format!("{} = {} as u64;", mmx_reg(reg), expr);
            }
        }
        self.set_op(instr, n, expr)
    }

    pub fn codegen_mmx(&mut self, instr: &iced_x86::Instruction) -> bool {
        use iced_x86::Mnemonic::*;
        // The PSLL/PSRL/PSRA and PUNPCK* mnemonics are shared between MMX and
        // SSE2. MMX forms operate on an MMX destination; SSE2 forms use an XMM
        // register.
        if matches!(
            instr.mnemonic(),
            Psllw
                | Pslld
                | Psllq
                | Psrlw
                | Psrld
                | Psrlq
                | Psraw
                | Psrad
                | Punpckhbw
                | Punpckhwd
                | Punpckhdq
                | Punpcklbw
                | Punpcklwd
                | Punpckldq
                | Paddb
                | Paddw
                | Paddd
                | Paddq
                | Psubb
                | Psubw
                | Psubd
                | Psubq
                | Paddsb
                | Paddsw
                | Paddusb
                | Paddusw
                | Psubsb
                | Psubsw
                | Psubusb
                | Psubusw
                | Pand
                | Pandn
                | Por
                | Pxor
                | Pcmpeqb
                | Pcmpeqw
                | Pcmpeqd
                | Pcmpgtb
                | Pcmpgtw
                | Pcmpgtd
                | Pavgb
                | Pavgw
                | Pminub
                | Pminsw
                | Pmaxub
                | Pmaxsw
                | Packsswb
                | Packssdw
                | Packuswb
                | Pmullw
                | Pmaddwd
                | Pmulhw
                | Pmulhuw
                | Pmuludq
                | Psadbw
        ) && !is_mmx_reg(instr.op_register(0))
        {
            return false;
        }
        // PMOVMSKB/PEXTRW read an MMX source in their MMX form and an XMM
        // source in their SSE2 form. PINSRW writes an MMX destination in its
        // MMX form and an XMM destination in its SSE2 form.
        if matches!(instr.mnemonic(), Pmovmskb | Pextrw) && !is_mmx_reg(instr.op_register(1)) {
            return false;
        }
        if matches!(instr.mnemonic(), Pinsrw) && !is_mmx_reg(instr.op_register(0)) {
            return false;
        }
        // MOVD and MOVQ have both MMX and SSE2 forms; let the XMM code generator
        // handle any encoding that touches an XMM register.
        if matches!(instr.mnemonic(), Movd | Movq)
            && (0..=1).any(|i| {
                matches!(instr.op_kind(i), iced_x86::OpKind::Register)
                    && codegen::xmm::is_xmm_reg(instr.op_register(i))
            })
        {
            return false;
        }
        match instr.mnemonic() {
            Movd => self.line(self.mmx_set_32(instr, 0, self.mmx_get_32(instr, 1))),
            // MOVNTQ is a non-temporal MMX store; in the flat memory model it
            // is identical to a regular 64-bit MMX store.
            Movq | Movntq => self.line(self.mmx_set(instr, 0, self.mmx_get(instr, 1))),
            // MOVDQ2Q copies the low qword of an XMM register into an MMX
            // register; the source is an XMM, so the guard did not kick in.
            Movdq2q => self.line(self.mmx_set(
                instr,
                0,
                format!("movq_from_xmm({})", self.get_op(instr, 1)),
            )),

            Pxor => {
                self.line(self.mmx_set(
                    instr,
                    0,
                    format!("{} ^ {}", self.mmx_get(instr, 0), self.mmx_get(instr, 1)),
                ));
            }
            Pand => {
                self.line(self.mmx_set(
                    instr,
                    0,
                    format!("{} & {}", self.mmx_get(instr, 0), self.mmx_get(instr, 1)),
                ));
            }
            Pandn => {
                self.line(self.mmx_set(
                    instr,
                    0,
                    format!("!{} & {}", self.mmx_get(instr, 0), self.mmx_get(instr, 1)),
                ));
            }
            Por => {
                self.line(self.mmx_set(
                    instr,
                    0,
                    format!("{} | {}", self.mmx_get(instr, 0), self.mmx_get(instr, 1)),
                ));
            }

            // Binary operations, all implemented with same name as mnemonic.
            Paddb | Paddd | Paddsb | Paddsw | Paddusb | Paddw | Pmullw | Pmaddwd | Psrlw
            | Psrld | Psrlq | Psllw | Pslld | Psllq | Psraw | Psrad | Packuswb | Packsswb
            | Packssdw | Pcmpeqb | Pcmpeqw | Pcmpeqd | Pcmpgtb | Pcmpgtw | Pcmpgtd | Punpckhbw
            | Punpckhwd | Punpckhdq | Psubb | Psubd | Psubsb | Psubsw | Psubusb | Psubw | Pavgb
            | Pavgw | Pminsw | Pminub | Pmaxsw | Pmaxub | Psadbw | Pmulhw | Pmulhuw | Pmuludq
            | Pavgusb | Pmulhrw => {
                let func = instr_name(instr);
                self.line(self.mmx_set(
                    instr,
                    0,
                    format!(
                        "{func}({}, {})",
                        self.mmx_get(instr, 0),
                        self.mmx_get(instr, 1)
                    ),
                ));
            }

            Pswapd => {
                self.line(self.mmx_set(instr, 0, format!("pswapd({})", self.mmx_get(instr, 1))))
            }

            Femms => {
                self.line("// no-op");
            }

            Pmovmskb => {
                // Destination is a GPR, not an MMX register.
                self.line(self.set_op(instr, 0, format!("pmovmskb({})", self.mmx_get(instr, 1))));
            }
            // PEXTRW's destination is a GPR, not an MMX register.
            Pextrw => {
                self.line(self.set_op(
                    instr,
                    0,
                    format!(
                        "pextrw({}, {})",
                        self.mmx_get(instr, 1),
                        self.get_op(instr, 2)
                    ),
                ));
            }
            // PINSRW's source is the low word of a GPR or a 16-bit memory read.
            Pinsrw => {
                self.line(self.mmx_set(
                    instr,
                    0,
                    format!(
                        "pinsrw({}, {} as u16, {})",
                        self.mmx_get(instr, 0),
                        self.get_op(instr, 1),
                        self.get_op(instr, 2)
                    ),
                ));
            }
            Pshufw => {
                self.line(self.mmx_set(
                    instr,
                    0,
                    format!(
                        "pshufw({}, {})",
                        self.mmx_get(instr, 1),
                        self.get_op(instr, 2)
                    ),
                ));
            }

            // MASKMOVQ's destination is the implicit DS:(E)DI operand; the
            // first register carries the data and the second the byte mask.
            Maskmovq => self.line(format!(
                "ctx.maskmovq({}, {});",
                self.mmx_get(instr, 1),
                self.mmx_get(instr, 2)
            )),

            // The low unpacks only read 4 bytes of a memory source.
            Punpcklbw | Punpcklwd | Punpckldq => {
                let func = instr_name(instr);
                self.line(self.mmx_set(
                    instr,
                    0,
                    format!(
                        "{func}({}, {})",
                        self.mmx_get_32(instr, 0),
                        self.mmx_get_32(instr, 1)
                    ),
                ));
            }

            Emms => {
                self.line("// no-op");
            }

            _ => return false,
        }
        true
    }
}
