use crate::codegen::{CodeGen, get_reg, instr_name, op_size};

impl<'a> CodeGen<'a> {
    /// Emit the tail of a divide-error (#DE, vector 0) trap. Real-mode code
    /// dispatches through the DOS interrupt vector; flat code has no IDT, so
    /// the trap is an explicit failure like other unhandled interrupts.
    fn divide_error(&self, instr: &iced_x86::Instruction) -> String {
        if self.module.is_dos() {
            // #DE is a fault: the pushed return address is the faulting
            // instruction itself.
            format!("return dos::int(ctx, {:#x}, 0x0)", instr.ip16())
        } else {
            format!("unhandled_interrupt(0x0, {:#x})", instr.ip32())
        }
    }

    pub fn codegen_math(&mut self, instr: &iced_x86::Instruction) -> bool {
        use iced_x86::Mnemonic::*;
        match instr.mnemonic() {
            // Binary operations.
            And | Or | Add | Sub | Sbb | Xor | Shl | Sal | Shr | Sar | Rol | Ror | Rcl | Rcr => {
                assert_eq!(instr.op_count(), 2);
                let func = if instr.mnemonic() == Sal {
                    "shl".to_string()
                } else {
                    instr_name(instr)
                };
                let op0 = self.get_op(instr, 0);
                let op1 = self.get_op(instr, 1);
                self.line(self.set_op(
                    instr,
                    0,
                    format!("{func}({op0}, {op1}, &mut ctx.cpu.flags)"),
                ));
            }

            Adc => {
                assert_eq!(instr.op_count(), 2);
                let op0 = self.get_op(instr, 0);
                let op1 = self.get_op(instr, 1);
                self.line("let carry = ctx.cpu.flags.contains(Flags::CF) as u32;");
                self.line(self.set_op(
                    instr,
                    0,
                    format!("addc({op0}, {op1}, carry as _, &mut ctx.cpu.flags)"),
                ));
            }

            Shld | Shrd => {
                assert_eq!(instr.op_count(), 3);
                let op0 = self.get_op(instr, 0);
                let op1 = self.get_op(instr, 1);
                let op2 = self.get_op(instr, 2);
                let op = match (instr.mnemonic(), op_size(instr, 0)) {
                    (Shld, 16) => "shld16",
                    (Shrd, 16) => "shrd16",
                    (Shld, 32) => "shld",
                    (Shrd, 32) => "shrd",
                    (mnemonic, size) => unreachable!("{mnemonic:?} with {size}-bit operand"),
                };
                self.line(self.set_op(
                    instr,
                    0,
                    format!("{op}({op0}, {op1}, {op2}, &mut ctx.cpu.flags)"),
                ));
            }

            Mul => {
                assert_eq!(instr.op_count(), 1);
                let size = op_size(instr, 0);
                let size2 = size * 2;
                let x = match size {
                    8 => get_reg(iced_x86::Register::AL),
                    16 => get_reg(iced_x86::Register::AX),
                    32 => get_reg(iced_x86::Register::EAX),
                    _ => unreachable!(),
                };
                self.line(format!(
                    "let res = mul({x} as u{size2}, {} as u{size2}, &mut ctx.cpu.flags);",
                    self.get_op(instr, 0)
                ));
                match size {
                    8 => self.line("ctx.cpu.regs.set_ax(res);"),
                    16 => self.line("ctx.cpu.regs.set_dx_ax(res);"),
                    32 => self.line("ctx.cpu.regs.set_edx_eax(res);"),
                    _ => unreachable!(),
                }
            }

            Div => {
                assert_eq!(instr.op_count(), 1);
                let size = op_size(instr, 0);
                let size2 = size * 2;
                let x = match size {
                    8 => get_reg(iced_x86::Register::AX),
                    16 => "ctx.cpu.regs.get_dx_ax()".to_string(),
                    32 => "ctx.cpu.regs.get_edx_eax()".to_string(),
                    _ => unreachable!(),
                };
                let y = format!("{} as u{size2}", self.get_op(instr, 0));
                let trap = self.divide_error(instr);
                self.line(format!("let dividend = {x};"));
                self.line(format!("let divisor = {y};"));
                self.line(format!(
                    "if divisor == 0 || dividend / divisor > u{size}::MAX as u{size2} {{ {trap}; }}"
                ));
                self.line("let (quot, rem) = div(dividend, divisor);");
                match size {
                    8 => {
                        self.line("ctx.cpu.regs.set_al(quot as u8);");
                        self.line("ctx.cpu.regs.set_ah(rem as u8);");
                    }
                    16 => {
                        self.line("ctx.cpu.regs.set_ax(quot as u16);");
                        self.line("ctx.cpu.regs.set_dx(rem as u16);");
                    }
                    32 => {
                        self.line("ctx.cpu.regs.eax = quot as u32;");
                        self.line("ctx.cpu.regs.edx = rem as u32;");
                    }
                    _ => unreachable!(),
                }
            }

            Imul => {
                let size = op_size(instr, 0);
                if instr.op_count() == 1 {
                    // one-op imul has different in/out reg and overflow behavior from others
                    let x = match size {
                        8 => get_reg(iced_x86::Register::AL),
                        16 => get_reg(iced_x86::Register::AX),
                        32 => get_reg(iced_x86::Register::EAX),
                        _ => unreachable!(),
                    };
                    let y = self.get_op(instr, 0);
                    let res = format!("imul1_{size}({x}, {y}, &mut ctx.cpu.flags)");
                    match size {
                        8 => self.line(format!("ctx.cpu.regs.set_ax({res});")),
                        16 => self.line(format!("ctx.cpu.regs.set_dx_ax({res});")),
                        32 => self.line(format!("ctx.cpu.regs.set_edx_eax({res});")),
                        _ => unreachable!(),
                    }
                } else {
                    let (x, y) = match instr.op_count() {
                        2 => {
                            assert_eq!(op_size(instr, 0), op_size(instr, 1));
                            (self.get_op(instr, 0), self.get_op(instr, 1))
                        }
                        3 => {
                            assert_eq!(op_size(instr, 0), op_size(instr, 1));
                            assert_eq!(op_size(instr, 1), op_size(instr, 2));
                            (self.get_op(instr, 1), self.get_op(instr, 2))
                        }
                        _ => unreachable!(),
                    };
                    self.line(self.set_op(
                        instr,
                        0,
                        format!("imul2_{size}({x}, {y}, &mut ctx.cpu.flags)"),
                    ));
                }
            }

            Idiv => {
                assert_eq!(instr.op_count(), 1);
                let size = op_size(instr, 0);
                let size2 = size * 2;
                let x = match size {
                    8 => "ctx.cpu.regs.get_ax() as i16".to_string(),
                    16 => "ctx.cpu.regs.get_dx_ax() as i32".to_string(),
                    32 => "ctx.cpu.regs.get_edx_eax() as i64".to_string(),
                    _ => unreachable!(),
                };
                let y = format!("{} as i{size} as i{size2}", self.get_op(instr, 0));
                let trap = self.divide_error(instr);
                self.line(format!("let x = {x};"));
                self.line(format!("let y = {y};"));
                self.line(format!(
                    "let Some((quot, rem)) = x.checked_div(y).zip(x.checked_rem(y)) else {{ {trap}; }};"
                ));
                self.line(format!(
                    "if quot < i{size}::MIN as i{size2} || quot > i{size}::MAX as i{size2} {{ {trap}; }}"
                ));
                let quot = format!("quot as i{size} as u{size}");
                let rem = format!("rem as i{size} as u{size}");
                match size {
                    8 => {
                        self.line(format!("ctx.cpu.regs.set_al({quot});"));
                        self.line(format!("ctx.cpu.regs.set_ah({rem});"));
                    }
                    16 => {
                        self.line(format!("ctx.cpu.regs.set_ax({quot});"));
                        self.line(format!("ctx.cpu.regs.set_dx({rem});"));
                    }
                    32 => {
                        self.line(format!("ctx.cpu.regs.eax = {quot};"));
                        self.line(format!("ctx.cpu.regs.edx = {rem};"));
                    }
                    _ => unreachable!(),
                }
            }

            Neg => self.line(self.set_op(
                instr,
                0,
                format!("neg({}, &mut ctx.cpu.flags)", self.get_op(instr, 0)),
            )),

            Dec => self.line(self.set_op(
                instr,
                0,
                format!("dec({}, &mut ctx.cpu.flags)", self.get_op(instr, 0)),
            )),
            Inc => self.line(self.set_op(
                instr,
                0,
                format!("inc({}, &mut ctx.cpu.flags)", self.get_op(instr, 0)),
            )),

            _ => return false,
        }
        true
    }
}
