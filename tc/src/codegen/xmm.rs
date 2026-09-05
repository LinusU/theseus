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

            _ => return false,
        }
        true
    }
}
