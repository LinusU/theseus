use crate::{
    Instr,
    codegen::{CodeGen, get_reg, instr_name, is_memory_op, reg_size},
    gather::IP,
};

impl<'a> CodeGen<'a> {
    /// Codegen the Cont for a jump to an statically known address.
    fn resolve_jmp(&mut self, ip: IP, from: u32) -> String {
        self.resolve_cont(ip.to_addr(), from)
    }

    fn jmp_target(&mut self, instr: &Instr) -> (Option<String>, Option<String>, String) {
        assert_eq!(instr.iced.op_count(), 1);
        let mut extra: Option<String> = None;
        let mut seg: Option<String> = None;
        let cont: String;
        match instr.iced.op0_kind() {
            iced_x86::OpKind::NearBranch16 => {
                let ip = instr.ip.with_local(instr.iced.near_branch16() as u32);
                cont = self.resolve_jmp(ip, instr.ip.to_addr());
            }
            iced_x86::OpKind::NearBranch32 => {
                let ip = instr.ip.with_local(instr.iced.near_branch32());
                cont = self.resolve_jmp(ip, instr.ip.to_addr());
            }
            iced_x86::OpKind::FarBranch16 => {
                let ip =
                    IP::Seg((instr.iced.far_branch_selector(), instr.iced.far_branch16()).into());
                seg = Some(format!("{:#x}", instr.iced.far_branch_selector()));
                cont = self.resolve_jmp(ip, instr.ip.to_addr());
            }
            iced_x86::OpKind::FarBranch32 => {
                let ip = IP::Flat(instr.iced.far_branch32());
                seg = Some(format!("{:#x}", instr.iced.far_branch_selector()));
                cont = self.resolve_jmp(ip, instr.ip.to_addr());
            }
            k if is_memory_op(k) => {
                // If it's like `jmp [someaddr]` where someaddr is in the IAT, resolve it directly.
                // (Note that `call [someaddr@IAT]` is generated as a direct function call.)
                if let Some(func) = &instr.hint {
                    return (None, None, format!("Cont({func})"));
                }

                let addr = self.gen_addr(&instr.iced);
                match instr.iced.memory_size() {
                    iced_x86::MemorySize::SegPtr16 => {
                        extra = Some(format!("let addr = ctx.memory.read::<SegOfs>({addr});"));
                        seg = Some("addr.seg".into());
                        cont = "ctx.indirect16(addr)".into();
                    }
                    iced_x86::MemorySize::WordOffset => {
                        extra = Some(format!("let addr = ctx.memory.read::<u16>({addr});"));
                        cont = if self.module.bitness() == 16 {
                            "ctx.indirect16((ctx.cpu.regs.cs, addr).into())".into()
                        } else {
                            // A 16-bit operand in flat code is an offset in
                            // the flat code segment, not a segment pair.
                            "ctx.indirect(addr as u32)".into()
                        };
                    }
                    iced_x86::MemorySize::DwordOffset => {
                        extra = Some(format!("let addr = ctx.memory.read::<u32>({addr});"));
                        cont = "ctx.indirect32(addr)".into();
                    }
                    iced_x86::MemorySize::SegPtr32 => {
                        extra = Some(format!(
                            "let addr = ctx.memory.read::<u32>({addr}); let seg = ctx.memory.read::<u16>(addr.wrapping_add(4u32));"
                        ));
                        seg = Some("seg".into());
                        cont = "ctx.indirect32(addr)".into();
                    }
                    s => cont = format!("todo!(\"{:?}\")", s),
                }
            }
            iced_x86::OpKind::Register => {
                let reg = instr.iced.op0_register();
                let expr = get_reg(reg);
                if self.module.bitness() == 16 {
                    cont = format!("ctx.indirect16((ctx.cpu.regs.cs, {expr}).into())");
                } else if reg_size(reg) == 32 {
                    cont = format!("ctx.indirect({expr})");
                } else {
                    // The operand-size prefix can select a 16-bit register in
                    // flat code, so zero-extend to the flat address width.
                    cont = format!("ctx.indirect({expr} as u32)");
                }
            }
            k => todo!("{:?}", k),
        }
        (extra, seg, cont)
    }

    pub fn codegen_control_flow(&mut self, instr: &Instr) -> bool {
        use iced_x86::Mnemonic::*;
        match instr.iced.mnemonic() {
            Jmp => {
                let (extra, seg, cont) = self.jmp_target(instr);
                if let Some(extra) = extra {
                    self.line(extra);
                }
                if let Some(seg) = seg {
                    self.line(format!("ctx.cpu.regs.cs = {seg};"));
                }
                self.line(cont);
            }
            Call => {
                if let Some(func) = &instr.hint {
                    self.line(format!(
                        "ctx.call_builtin({:#x}, {func});",
                        instr.next_ip().local()
                    ));
                } else {
                    let (extra, seg, cont) = self.jmp_target(instr);
                    if let Some(extra) = extra {
                        self.line(extra);
                    }
                    // The operand-size prefix, not the module bitness, decides
                    // how many bytes of return address a call pushes.
                    let bitness = match instr.iced.code() {
                        iced_x86::Code::Call_rel16
                        | iced_x86::Code::Call_rm16
                        | iced_x86::Code::Call_ptr1616
                        | iced_x86::Code::Call_m1616 => 16,
                        _ => 32,
                    };
                    let ip = instr.next_ip().local();
                    let ret = if bitness == 16 {
                        format!("{ip:#x}u32 as u16")
                    } else {
                        format!("{ip:#x}")
                    };
                    if let Some(seg) = seg {
                        self.line(format!("ctx.callf{bitness}({ret}, {seg}, {cont})"));
                    } else {
                        self.line(format!("ctx.call{bitness}({ret}, {cont})"));
                    }
                }
            }
            Ret | Retf => {
                let n = match instr.iced.op_count() {
                    0 => 0,
                    1 => {
                        assert!(instr.iced.op0_kind() == iced_x86::OpKind::Immediate16);
                        instr.iced.immediate16()
                    }
                    _ => todo!(),
                };
                // The operand-size prefix selects a 16-bit return even in a
                // 32-bit module, and vice versa in a 16-bit module.
                let bitness = match instr.iced.code() {
                    iced_x86::Code::Retnw
                    | iced_x86::Code::Retnw_imm16
                    | iced_x86::Code::Retfw
                    | iced_x86::Code::Retfw_imm16 => 16,
                    _ => 32,
                };
                self.line(format!(
                    "ctx.{name}{bitness}({n})",
                    name = instr_name(&instr.iced),
                ));
            }
            Iret | Iretd => {
                let bitness = match instr.iced.code() {
                    iced_x86::Code::Iretw => 16,
                    _ => 32,
                };
                self.line(format!("ctx.iret{bitness}()"));
            }
            Into => {
                let next = self.resolve_jmp(instr.next_ip(), instr.ip.to_addr());
                self.line("if ctx.cpu.flags.contains(Flags::OF) {");
                if self.module.is_dos() {
                    // INTO is a trap: the pushed return address is the
                    // following instruction.
                    self.line(format!(
                        "return dos::int(ctx, {:#x}, 0x4);",
                        instr.next_ip().local()
                    ));
                } else {
                    self.line(format!(
                        "unhandled_interrupt(0x4, {:#x});",
                        instr.iced.ip32()
                    ));
                }
                self.line("}");
                self.line(next);
            }
            Je | Jne | Jb | Js | Jns | Jo | Jno | Jp | Jnp | Ja | Jae | Jl | Jg | Jge | Jecxz
            | Jle | Jbe | Jcxz | Loop | Loope | Loopne => {
                let next = self.resolve_jmp(instr.next_ip(), instr.ip.to_addr());
                let (None, None, cont) = self.jmp_target(instr) else {
                    panic!()
                };
                let func = instr_name(&instr.iced);
                self.line(format!("ctx.{func}({next}, {cont})"));
            }

            _ => return false,
        }
        true
    }
}
