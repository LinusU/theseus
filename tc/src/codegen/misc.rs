use crate::codegen::{CodeGen, get_mem, instr_name, op_size};

impl<'a> CodeGen<'a> {
    pub fn codegen_misc(&mut self, instr: &iced_x86::Instruction) -> bool {
        use iced_x86::Mnemonic::*;
        match instr.mnemonic() {
            Push => {
                let func = match op_size(instr, 0) {
                    16 => "push16",
                    32 => "push32",
                    _ => return false,
                };
                self.line(format!("ctx.{func}({});", self.get_op(instr, 0)));
            }
            Pop => {
                let func = match op_size(instr, 0) {
                    16 => "pop16",
                    32 => "pop32",
                    _ => return false,
                };
                self.line(format!("let x = ctx.{func}();"));
                self.line(self.set_op(instr, 0, "x".into()))
            }
            Pushad => self.line("ctx.pushad();"),
            Popad => self.line("ctx.popad();"),
            Mov => self.line(self.set_op(instr, 0, self.get_op(instr, 1))),

            Sete | Setne | Setg | Setge | Setl | Setle | Seta | Setae | Setb | Setbe | Seto
            | Setno | Sets | Setns | Setp | Setnp => {
                self.line(self.set_op(instr, 0, format!("ctx.{}()", instr_name(instr))))
            }

            Cmp => {
                let op0 = self.get_op(instr, 0);
                let op1 = self.get_op(instr, 1);
                self.line(format!("sub({op0}, {op1}, &mut ctx.cpu.flags);"));
            }
            Test => {
                self.line(format!(
                    "and({}, {}, &mut ctx.cpu.flags);",
                    self.get_op(instr, 0),
                    self.get_op(instr, 1)
                ));
            }

            Lea => {
                // Note: in 16-bit mode lea ignores segment registers.
                let addr = self.gen_addr_offset(instr);
                self.line(self.set_op(instr, 0, addr));
            }

            Movzx => {
                self.line(self.set_op(instr, 0, format!("{} as _", self.get_op(instr, 1))));
            }
            Movsx => {
                let read = format!(
                    "{read} as i{src} as i{dst} as u{dst}",
                    read = self.get_op(instr, 1),
                    src = op_size(instr, 1),
                    dst = op_size(instr, 0)
                );
                self.line(self.set_op(instr, 0, read));
            }

            Leave => self.line("ctx.leave();"),
            Enter => {
                assert!(instr.op1_kind() == iced_x86::OpKind::Immediate8_2nd);
                let op1 = instr.immediate8_2nd();
                self.line(format!("ctx.enter({}, {:x});", self.get_op(instr, 0), op1));
            }

            Xchg => {
                self.line(format!("let t = {};", self.get_op(instr, 0)));
                self.line(self.set_op(instr, 0, self.get_op(instr, 1)));
                self.line(self.set_op(instr, 1, "t".into()));
            }
            Nop => {}
            // x87 exception sync; our FPU never raises.
            Wait => {}

            Not => self.line(self.set_op(instr, 0, format!("!{}", self.get_op(instr, 0)))),

            Bt | Bts | Btr | Btc => {
                let size = op_size(instr, 0);
                assert!(matches!(size, 16 | 32));
                let operation = instr_name(instr);
                let bit = self.get_op(instr, 1);
                self.line(format!("let bit = ({bit}) as u32;"));
                if instr.op_kind(0) == iced_x86::OpKind::Memory {
                    let addr = self.gen_addr(instr);
                    let addr = if instr.op_kind(1) == iced_x86::OpKind::Register {
                        let mask = !(size as u32 - 1);
                        let bytes = size / 8;
                        format!("{addr}.wrapping_add((bit & {mask:#x}u32) / {bytes}u32)")
                    } else {
                        addr
                    };
                    self.line(format!("let addr = {addr};"));
                    self.line(format!("let value = ctx.memory.read::<u{size}>(addr);"));
                    if operation == "bt" {
                        self.line(format!("bt(value, bit, &mut ctx.cpu.flags);"));
                    } else {
                        self.line(format!(
                            "ctx.memory.write::<u{size}>(addr, {operation}(value, bit, &mut ctx.cpu.flags));"
                        ));
                    }
                } else if operation == "bt" {
                    self.line(format!(
                        "bt({}, bit, &mut ctx.cpu.flags);",
                        self.get_op(instr, 0)
                    ));
                } else {
                    self.line(self.set_op(
                        instr,
                        0,
                        format!(
                            "{operation}({}, bit, &mut ctx.cpu.flags)",
                            self.get_op(instr, 0)
                        ),
                    ));
                }
            }

            Int => {
                assert!(instr.op0_kind() == iced_x86::OpKind::Immediate8);
                // A misidentified code pointer can land us on an `int` in
                // non-DOS code, where there's nothing to call.
                if self.module.is_dos() {
                    self.line(format!(
                        "dos::int(ctx, {:#x}, {:#x})",
                        instr.next_ip32(),
                        instr.immediate8()
                    ));
                } else {
                    self.line(format!(
                        "unhandled_interrupt({:#x}, {:#x});",
                        instr.immediate8(),
                        instr.ip32()
                    ));
                }
            }
            Rdtsc => self.line("ctx.cpu.regs.set_edx_eax(rdtsc());"),
            Int1 | Int3 => {}
            Pushfd => self.line("ctx.push32(ctx.cpu.flags.bits() | 2);"),
            Popfd => self.line("ctx.cpu.flags = Flags::from_bits_truncate(ctx.pop32());"),
            Cpuid => {
                self.line(
                    "let (cpuid_eax, cpuid_ebx, cpuid_ecx, cpuid_edx) = cpuid(ctx.cpu.regs.eax, ctx.cpu.regs.ecx);",
                );
                self.line("ctx.cpu.regs.eax = cpuid_eax;");
                self.line("ctx.cpu.regs.ebx = cpuid_ebx;");
                self.line("ctx.cpu.regs.ecx = cpuid_ecx;");
                self.line("ctx.cpu.regs.edx = cpuid_edx;");
            }
            Cmpxchg => {
                let (accumulator, set_accumulator) = match op_size(instr, 0) {
                    8 => ("ctx.cpu.regs.get_al()", "ctx.cpu.regs.set_al(cmpxchg_old);"),
                    16 => ("ctx.cpu.regs.get_ax()", "ctx.cpu.regs.set_ax(cmpxchg_old);"),
                    32 => ("ctx.cpu.regs.eax", "ctx.cpu.regs.eax = cmpxchg_old;"),
                    size => unreachable!("{size}"),
                };
                let old = self.get_op(instr, 0);
                let source = self.get_op(instr, 1);
                self.line("{");
                self.line(format!("let cmpxchg_old = {old};"));
                self.line(format!(
                    "sub({accumulator}, cmpxchg_old, &mut ctx.cpu.flags);"
                ));
                self.line("if ctx.cpu.flags.contains(Flags::ZF) {");
                self.line(self.set_op(instr, 0, source));
                self.line("} else {");
                self.line(set_accumulator);
                self.line("}");
                self.line("}");
            }
            Xgetbv => {
                self.line("let (xgetbv_eax, xgetbv_edx) = xgetbv(ctx.cpu.regs.ecx);");
                self.line("ctx.cpu.regs.eax = xgetbv_eax;");
                self.line("ctx.cpu.regs.edx = xgetbv_edx;");
            }
            Div => self.todo(instr_name(instr)),

            // CBW/CWDE: sign extend to next larger ax
            Cbw => self.line("ctx.cpu.regs.set_ax(ctx.cpu.regs.get_al() as i8 as i16 as u16);"),
            Cwde => self.line("ctx.cpu.regs.eax = ctx.cpu.regs.get_ax() as i16 as i32 as u32;"),

            // CWD/CDQ: sign extend to dx:ax
            Cwd => self.line("ctx.cpu.regs.set_dx_ax(ctx.cpu.regs.get_ax() as i16 as i32 as u32);"),
            Cdq => self.line("ctx.cpu.regs.set_edx_eax(ctx.cpu.regs.eax as i32 as i64 as u64);"),

            Stc | Clc | Std | Cld | Sahf => {
                self.line(format!("{}(ctx);", instr_name(instr)));
            }

            Cli | Sti => {
                self.line(format!("ctx.{}();", instr_name(instr)));
            }

            In => {
                assert_eq!(instr.op_count(), 2);
                let port = if instr.op1_kind() == iced_x86::OpKind::Immediate8 {
                    format!("{:#x}u16", instr.immediate8())
                } else {
                    self.get_op(instr, 1)
                };
                let width = op_size(instr, 0);
                self.line(self.set_op(instr, 0, format!("port_in({port}, {width}) as u{width}")));
            }
            Out => {
                assert_eq!(instr.op_count(), 2);
                let port = if instr.op0_kind() == iced_x86::OpKind::Immediate8 {
                    // The Imm8 form can only reference the first 256 ports, but otherwise it's the same call.
                    format!("{:#x}u16", instr.immediate8())
                } else {
                    self.get_op(instr, 0)
                };
                let width = op_size(instr, 1);
                if self.module.is_dos() {
                    self.line(format!("dos::out(ctx, {port}, {});", self.get_op(instr, 1)));
                } else {
                    self.line(format!(
                        "port_out({port}, ({}) as u32, {width});",
                        self.get_op(instr, 1)
                    ));
                }
            }

            Lds | Les | Lfs | Lgs | Lss => {
                assert_eq!(instr.op_count(), 2);
                let segment = match instr.mnemonic() {
                    Lds => "ds",
                    Les => "es",
                    Lfs => "fs",
                    Lgs => "gs",
                    Lss => "ss",
                    _ => unreachable!(),
                };
                let address = self.gen_addr(instr);
                if self.module.bitness() == 16 {
                    self.line(format!("let ptr = {};", get_mem("u32".into(), address),));
                    self.line(format!("ctx.cpu.regs.{segment} = (ptr >> 16) as u16;"));
                    self.line(self.set_op(instr, 0, "ptr as u16".into()));
                } else {
                    self.line(format!("let ptr = {};", get_mem("u64".into(), address),));
                    self.line(format!("ctx.cpu.regs.{segment} = (ptr >> 32) as u16;"));
                    self.line(self.set_op(instr, 0, "ptr as u32".into()));
                }
            }

            Xlatb => self.line("ctx.xlat();"),

            _ => return false,
        }
        true
    }
}
