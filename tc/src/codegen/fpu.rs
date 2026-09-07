use crate::codegen::{CodeGen, get_mem, instr_name, is_memory_op, mem_size, op_size};

fn reg_to_index(register: iced_x86::Register) -> usize {
    use iced_x86::Register::*;
    match register {
        ST0 => 0,
        ST1 => 1,
        ST2 => 2,
        ST3 => 3,
        ST4 => 4,
        ST5 => 5,
        ST6 => 6,
        ST7 => 7,
        r => panic!("unhandled FPU register in reg_to_index: {r:?}"),
    }
}

fn is_16_bit_fpu_state(instr: &iced_x86::Instruction) -> bool {
    matches!(
        instr.memory_size(),
        iced_x86::MemorySize::FpuEnv14 | iced_x86::MemorySize::FpuState94
    )
}

impl<'a> CodeGen<'a> {
    fn fpu_get_mem(&self, instr: &iced_x86::Instruction) -> String {
        let addr = self.gen_addr(instr);
        if instr.memory_size() == iced_x86::MemorySize::Float80 {
            format!("ctx.memory.read::<F80>({addr}).to_f64()")
        } else {
            let size = mem_size(instr);
            if size != 64 {
                format!("{} as f64", get_mem(format!("f{size}"), addr))
            } else {
                get_mem(format!("f{size}"), addr)
            }
        }
    }

    fn fpu_set_mem(&self, instr: &iced_x86::Instruction, expr: String) -> String {
        // TODO: is this only needed by fst?
        let addr = self.gen_addr(instr);
        if instr.memory_size() == iced_x86::MemorySize::Float80 {
            format!("ctx.memory.write::<F80>({addr}, F80::from_f64({expr}));")
        } else {
            let size = mem_size(instr);
            format!("ctx.memory.write::<f{size}>({addr}, {expr});")
        }
    }

    fn fpu_get_reg(&self, index: usize) -> String {
        format!("ctx.cpu.fpu.get({index})")
    }

    fn fpu_set_reg(&self, index: usize, expr: String) -> String {
        format!("ctx.cpu.fpu.set({index}, {expr});")
    }

    /// `to_int` already returns `u64`, so only the 16- and 32-bit stores need a cast.
    fn fpu_to_int_expr(&self, reg: String, truncate: bool, size: usize) -> String {
        if size == 64 {
            format!("ctx.cpu.fpu.to_int({reg}, {truncate}, {size})")
        } else {
            format!("ctx.cpu.fpu.to_int({reg}, {truncate}, {size}) as u{size}")
        }
    }

    fn fpu_get_op(&self, instr: &iced_x86::Instruction, n: u32) -> String {
        use iced_x86::OpKind::*;
        let kind = instr.op_kind(n);
        if is_memory_op(kind) {
            self.fpu_get_mem(instr)
        } else if kind == Register {
            self.fpu_get_reg(reg_to_index(instr.op_register(n)))
        } else {
            panic!("unhandled FPU source operand kind: {kind:?}")
        }
    }

    fn fpu_set_op(&self, instr: &iced_x86::Instruction, n: u32, expr: String) -> String {
        use iced_x86::OpKind::*;
        let kind = instr.op_kind(n);
        if is_memory_op(kind) {
            let size = mem_size(instr);
            let expr = if size != 64 && size != 80 {
                format!("{expr} as f{size}")
            } else {
                expr
            };
            self.fpu_set_mem(instr, expr)
        } else if kind == Register {
            self.fpu_set_reg(reg_to_index(instr.op_register(n)), expr)
        } else {
            panic!("unhandled FPU destination operand kind: {kind:?}")
        }
    }

    pub fn codegen_fpu(&mut self, instr: &iced_x86::Instruction) -> bool {
        use iced_x86::Mnemonic::*;
        match instr.mnemonic() {
            Fld => {
                let expr = self.fpu_get_op(instr, 0);
                self.line(format!("ctx.cpu.fpu.push({expr});"));
            }
            Fild => {
                self.line(format!(
                    "ctx.cpu.fpu.push({} as i{size} as f64);",
                    self.get_op(instr, 0),
                    size = op_size(instr, 0)
                ));
            }
            Fldz => self.line("ctx.cpu.fpu.push(0.0);"),
            Fld1 => self.line("ctx.cpu.fpu.push(1.0);"),
            Fldl2e => self.line("ctx.cpu.fpu.push(std::f64::consts::LOG2_E);"),
            Fldl2t => self.line("ctx.cpu.fpu.push(std::f64::consts::LOG2_10);"),
            Fldln2 => self.line("ctx.cpu.fpu.push(std::f64::consts::LN_2);"),
            Fldpi => self.line("ctx.cpu.fpu.push(std::f64::consts::PI);"),
            Fldlg2 => self.line("ctx.cpu.fpu.push(std::f64::consts::LOG10_2);"),

            Fst | Fstp => {
                self.line(self.fpu_set_op(instr, 0, self.fpu_get_reg(0)));
                if instr.mnemonic() == Fstp {
                    self.line("ctx.cpu.fpu.pop();");
                }
            }

            Fist | Fistp => {
                let size = op_size(instr, 0);
                self.line(self.set_op(
                    instr,
                    0,
                    self.fpu_to_int_expr(self.fpu_get_reg(0), false, size),
                ));
                if instr.mnemonic() == Fistp {
                    self.line("ctx.cpu.fpu.pop();");
                }
            }
            Fisttp => {
                let size = op_size(instr, 0);
                self.line(self.set_op(
                    instr,
                    0,
                    self.fpu_to_int_expr(self.fpu_get_reg(0), true, size),
                ));
                self.line("ctx.cpu.fpu.pop();");
            }

            Fbld => {
                self.line(format!(
                    "ctx.cpu.fpu.bld(&ctx.memory, {});",
                    self.gen_addr(instr)
                ));
            }
            Fbstp => {
                self.line(format!(
                    "ctx.cpu.fpu.bstp(&mut ctx.memory, {});",
                    self.gen_addr(instr)
                ));
            }

            // Binary ops
            Fadd | Faddp | Fsub | Fsubp | Fsubr | Fsubrp | Fmul | Fmulp | Fdivp | Fdivrp
            | Fdivr | Fdiv => {
                assert!(matches!(instr.op_count(), 1 | 2));

                let (arg0, arg1) = if instr.op_count() == 1 {
                    (self.fpu_get_reg(0), self.fpu_get_op(instr, 0))
                } else {
                    (self.fpu_get_op(instr, 0), self.fpu_get_op(instr, 1))
                };

                let (arg0, arg1) = if matches!(instr.mnemonic(), Fsubr | Fsubrp | Fdivr | Fdivrp) {
                    (arg1, arg0)
                } else {
                    (arg0, arg1)
                };

                let binop = match instr.mnemonic() {
                    Fadd | Faddp => "+",
                    Fsub | Fsubp | Fsubr | Fsubrp => "-",
                    Fmul | Fmulp => "*",
                    Fdiv | Fdivp | Fdivr | Fdivrp => "/",
                    _ => unreachable!(),
                };

                let expr = format!("{arg0} {binop} {arg1}");

                if instr.op_count() == 1 {
                    self.line(self.fpu_set_reg(0, expr));
                } else {
                    self.line(self.fpu_set_op(instr, 0, expr));
                }

                if matches!(
                    instr.mnemonic(),
                    Faddp | Fsubp | Fsubrp | Fmulp | Fdivp | Fdivrp
                ) {
                    self.line("ctx.cpu.fpu.pop();");
                }
            }

            Fimul => {
                let size = op_size(instr, 0);
                let expr = format!(
                    "{} * {} as i{size} as f64",
                    self.fpu_get_reg(0),
                    self.get_op(instr, 0)
                );
                self.line(self.fpu_set_reg(0, expr));
            }
            Fiadd | Fisub | Fisubr => {
                let size = op_size(instr, 0);
                let int = format!("{} as i{size} as f64", self.get_op(instr, 0));
                let st = self.fpu_get_reg(0);
                let expr = match instr.mnemonic() {
                    Fiadd => format!("{st} + {int}"),
                    Fisub => format!("{st} - {int}"),
                    Fisubr => format!("{int} - {st}"),
                    _ => unreachable!(),
                };
                self.line(self.fpu_set_reg(0, expr));
            }
            Fidiv | Fidivr => {
                let size = op_size(instr, 0);
                let operand = format!("{} as i{size} as f64", self.get_op(instr, 0));
                let expr = if instr.mnemonic() == Fidiv {
                    format!("{} / {operand}", self.fpu_get_reg(0))
                } else {
                    format!("{operand} / {}", self.fpu_get_reg(0))
                };
                self.line(self.fpu_set_reg(0, expr));
            }

            Fprem => {
                self.line("ctx.cpu.fpu.prem(false);");
            }
            Fprem1 => {
                self.line("ctx.cpu.fpu.prem(true);");
            }

            Fchs => {
                self.line(self.fpu_set_reg(0, format!("-{}", self.fpu_get_reg(0))));
            }
            Fscale => {
                self.line(self.fpu_set_reg(
                    0,
                    format!(
                        "{} * 2f64.powi({}.trunc() as i32)",
                        self.fpu_get_reg(0),
                        self.fpu_get_reg(1)
                    ),
                ));
            }
            Frndint => {
                self.line(
                    self.fpu_set_reg(0, format!("ctx.cpu.fpu.round({})", self.fpu_get_reg(0))),
                );
            }
            Fabs => {
                self.line(self.fpu_set_reg(0, format!("{}.abs()", self.fpu_get_reg(0))));
            }

            F2xm1 => {
                self.line(self.fpu_set_reg(0, format!("{}.exp2() - 1.0", self.fpu_get_reg(0))));
            }
            Fptan => {
                self.line(self.fpu_set_reg(0, format!("{}.tan()", self.fpu_get_reg(0))));
                self.line("ctx.cpu.fpu.push(1.0);");
            }
            Fsin => {
                self.line(self.fpu_set_reg(0, format!("{}.sin()", self.fpu_get_reg(0))));
            }
            Fcos => {
                self.line(self.fpu_set_reg(0, format!("{}.cos()", self.fpu_get_reg(0))));
            }
            Fxtract => self.line("ctx.cpu.fpu.extract();"),
            Fsincos => {
                self.line("let fsincos_t = ctx.cpu.fpu.get(0);");
                self.line("ctx.cpu.fpu.set(0, fsincos_t.sin());");
                self.line("ctx.cpu.fpu.push(fsincos_t.cos());");
            }
            Fnop => {}
            Fdecstp => self.line("ctx.cpu.fpu.dec_top();"),
            Fincstp => self.line("ctx.cpu.fpu.inc_top();"),
            Ffree | Ffreep => {
                self.line(format!(
                    "ctx.cpu.fpu.{}({});",
                    instr_name(instr),
                    reg_to_index(instr.op_register(0))
                ));
            }
            Fsqrt => {
                self.line(self.fpu_set_reg(0, format!("{}.sqrt()", self.fpu_get_reg(0))));
            }

            Fxch => {
                assert_eq!(instr.op_count(), 2);
                let op0 = self.fpu_get_op(instr, 0);
                let op1 = self.fpu_get_op(instr, 1);
                self.line("{");
                self.line(format!("let (old_0, old_1) = ({op0}, {op1});"));
                self.line(self.fpu_set_op(instr, 0, "old_1".into()));
                self.line(self.fpu_set_op(instr, 1, "old_0".into()));
                self.line("}");
            }

            Ftst => {
                self.line("ctx.cpu.fpu.compare(ctx.cpu.fpu.get(0), 0.0);");
            }
            Fxam => {
                self.line("ctx.cpu.fpu.examine();");
            }
            Fcom | Fcomp | Fucom | Fucomp => {
                let (arg0, arg1) = match instr.op_count() {
                    1 => (self.fpu_get_reg(0), self.fpu_get_op(instr, 0)),
                    2 => (self.fpu_get_op(instr, 0), self.fpu_get_op(instr, 1)),
                    _ => unreachable!(),
                };
                self.line(format!("ctx.cpu.fpu.compare({arg0}, {arg1});"));
                if matches!(instr.mnemonic(), Fcomp | Fucomp) {
                    self.line("ctx.cpu.fpu.pop();");
                }
            }
            Fcompp | Fucompp => {
                self.line("ctx.cpu.fpu.compare(ctx.cpu.fpu.get(0), ctx.cpu.fpu.get(1));");
                self.line("ctx.cpu.fpu.pop();");
                self.line("ctx.cpu.fpu.pop();");
            }
            Ficom | Ficomp => {
                let size = op_size(instr, 0);
                self.line(format!(
                    "ctx.cpu.fpu.compare(ctx.cpu.fpu.get(0), {} as i{size} as f64);",
                    self.get_op(instr, 0)
                ));
                if instr.mnemonic() == Ficomp {
                    self.line("ctx.cpu.fpu.pop();");
                }
            }
            // FCOMI family reports through EFLAGS (ZF/PF/CF), not the FPU
            // status word.
            Fcomi | Fcomip | Fucomi | Fucomip => {
                assert_eq!(instr.op_count(), 2);
                self.line(format!(
                    "ctx.cpu.fpu.compare_flags({}, {}, &mut ctx.cpu.flags);",
                    self.fpu_get_op(instr, 0),
                    self.fpu_get_op(instr, 1)
                ));
                if matches!(instr.mnemonic(), Fcomip | Fucomip) {
                    self.line("ctx.cpu.fpu.pop();");
                }
            }

            Fstsw | Fnstsw => {
                assert_eq!(instr.op_count(), 1);
                self.line(self.set_op(instr, 0, "ctx.cpu.fpu.status()".into()));
            }
            Fstenv | Fnstenv => {
                let func = if is_16_bit_fpu_state(instr) {
                    "store_env16"
                } else {
                    "store_env"
                };
                self.line(format!(
                    "ctx.cpu.fpu.{func}(&mut ctx.memory, {});",
                    self.gen_addr(instr)
                ));
            }
            Fldenv => {
                let func = if is_16_bit_fpu_state(instr) {
                    "load_env16"
                } else {
                    "load_env"
                };
                self.line(format!(
                    "ctx.cpu.fpu.{func}(&ctx.memory, {});",
                    self.gen_addr(instr)
                ));
            }
            Fsave | Fnsave => {
                let func = if is_16_bit_fpu_state(instr) {
                    "save16"
                } else {
                    "save"
                };
                self.line(format!(
                    "ctx.cpu.fpu.{func}(&mut ctx.memory, {});",
                    self.gen_addr(instr)
                ));
            }
            Frstor => {
                let func = if is_16_bit_fpu_state(instr) {
                    "restore16"
                } else {
                    "restore"
                };
                self.line(format!(
                    "ctx.cpu.fpu.{func}(&ctx.memory, {});",
                    self.gen_addr(instr)
                ));
            }
            Fxsave => {
                self.line(format!(
                    "ctx.cpu.fpu.fxsave(&mut ctx.memory, {});",
                    self.gen_addr(instr)
                ));
            }
            Fxrstor => {
                self.line(format!(
                    "ctx.cpu.fpu.fxrstor(&ctx.memory, {});",
                    self.gen_addr(instr)
                ));
            }

            // We don't model FPU exceptions, so clearing them is a no-op.
            Fclex | Fnclex => {}

            Finit | Fninit => self.line("ctx.cpu.fpu.init();"),

            Fstcw | Fnstcw => {
                assert_eq!(instr.op_count(), 1);
                self.line(self.set_op(instr, 0, "ctx.cpu.fpu.control".into()));
            }

            Fldcw => {
                assert_eq!(instr.op_count(), 1);
                self.line(format!("ctx.cpu.fpu.control = {};", self.get_op(instr, 0)));
            }

            Fpatan => {
                self.line("let t = ctx.cpu.fpu.get(0);");
                self.line("ctx.cpu.fpu.pop();");
                self.line("ctx.cpu.fpu.set(0, ctx.cpu.fpu.get(0).atan2(t));");
            }
            Fyl2x => {
                self.line("let t = ctx.cpu.fpu.get(1) * ctx.cpu.fpu.get(0).log2();");
                self.line("ctx.cpu.fpu.pop();");
                self.line("ctx.cpu.fpu.set(0, t);");
            }
            Fyl2xp1 => {
                self.line("let t = ctx.cpu.fpu.get(1) * (ctx.cpu.fpu.get(0) + 1.0).log2();");
                self.line("ctx.cpu.fpu.pop();");
                self.line("ctx.cpu.fpu.set(0, t);");
            }

            // FCMOVcc: conditional floating-point move on CPU flags.
            Fcmovb | Fcmovnb | Fcmove | Fcmovne | Fcmovbe | Fcmovnbe | Fcmovu | Fcmovnu => {
                assert_eq!(instr.op_count(), 2);
                let cond = match instr.mnemonic() {
                    Fcmovb => "ctx.cpu.flags.contains(Flags::CF)",
                    Fcmovnb => "!ctx.cpu.flags.contains(Flags::CF)",
                    Fcmove => "ctx.cpu.flags.contains(Flags::ZF)",
                    Fcmovne => "!ctx.cpu.flags.contains(Flags::ZF)",
                    Fcmovbe => "ctx.cpu.flags.intersects(Flags::CF | Flags::ZF)",
                    Fcmovnbe => "!ctx.cpu.flags.intersects(Flags::CF | Flags::ZF)",
                    Fcmovu => "ctx.cpu.flags.contains(Flags::PF)",
                    Fcmovnu => "!ctx.cpu.flags.contains(Flags::PF)",
                    _ => unreachable!(),
                };
                self.line(format!(
                    "if {cond} {{ ctx.cpu.fpu.set(0, {}); }}",
                    self.fpu_get_op(instr, 1)
                ));
            }
            _ => return false,
        }
        true
    }
}
