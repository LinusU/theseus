use crate::codegen::{CodeGen, get_mem, instr_name, is_memory_op, op_size, reg_name, reg_size};

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
            // PUSHA/POPA and PUSHAD/POPAD are distinct mnemonics keyed by
            // the operand-size attribute; the helpers push 16- or 32-bit
            // registers while push16/push32 handle the stack-pointer width.
            Pusha => self.line("ctx.pusha();"),
            Pushad => self.line("ctx.pushad();"),
            Popa => self.line("ctx.popa();"),
            Popad => self.line("ctx.popad();"),
            Mov => {
                // Moves to or from control, debug, or test registers are
                // privileged in Windows usermode, and on DOS would imply a
                // mode switch this machine cannot model.
                let privileged = instr.op_count() == 2
                    && [instr.op_register(0), instr.op_register(1)]
                        .iter()
                        .any(|r| r.is_cr() || r.is_dr() || r.is_tr());
                // A register moved onto itself is a semantic no-op; compilers
                // emit `mov edi,edi` as hot-patch padding, and codegen would
                // otherwise produce a self-assignment.
                let self_move = instr.op0_kind() == iced_x86::OpKind::Register
                    && instr.op1_kind() == iced_x86::OpKind::Register
                    && instr.op_register(0) == instr.op_register(1);
                if privileged {
                    self.line(format!("unhandled_interrupt(0xd, {:#x});", instr.ip32()));
                } else if !self_move {
                    self.line(self.set_op(instr, 0, self.get_op(instr, 1)))
                }
            }

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
                let dst = self.get_op(instr, 0);
                if addr != dst {
                    self.line(self.set_op(instr, 0, addr));
                }
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

            // ENTER/LEAVE share their mnemonics across operand sizes; the
            // iced code's w/d suffix picks the register and push width.
            Leave => {
                let func = if instr.code() == iced_x86::Code::Leavew {
                    "leave16"
                } else {
                    "leave"
                };
                self.line(format!("ctx.{func}();"));
            }
            Enter => {
                assert!(instr.op1_kind() == iced_x86::OpKind::Immediate8_2nd);
                let op1 = instr.immediate8_2nd();
                let func = if instr.code() == iced_x86::Code::Enterw_imm16_imm8 {
                    "enter16"
                } else {
                    "enter"
                };
                self.line(format!("ctx.{func}({}, {:x});", self.get_op(instr, 0), op1));
            }

            Xchg => {
                let op0 = self.get_op(instr, 0);
                let op1 = self.get_op(instr, 1);
                self.line("{");
                self.line(format!("let (old_0, old_1) = ({op0}, {op1});"));
                // If the memory operand depends on the other register, we must
                // write the memory value before changing the register.
                if is_memory_op(instr.op_kind(1)) {
                    self.line(self.set_op(instr, 1, "old_0".into()));
                    self.line(self.set_op(instr, 0, "old_1".into()));
                } else {
                    self.line(self.set_op(instr, 0, "old_1".into()));
                    self.line(self.set_op(instr, 1, "old_0".into()));
                }
                self.line("}");
            }
            Nop => {}
            // x87 exception sync; our FPU never raises.
            Wait => {}

            Not => self.line(self.set_op(instr, 0, format!("!{}", self.get_op(instr, 0)))),
            Bswap => self.line(self.set_op(instr, 0, format!("bswap({})", self.get_op(instr, 0)))),

            Bt | Bts | Btr | Btc => {
                let size = op_size(instr, 0);
                assert!(matches!(size, 16 | 32));
                let operation = instr_name(instr);
                let bit_op = self.get_op(instr, 1);
                let bit = if instr.op_kind(1) == iced_x86::OpKind::Register
                    && reg_size(instr.op_register(1)) == 32
                {
                    bit_op.clone()
                } else {
                    format!("{bit_op} as u32")
                };
                self.line(format!("let bit = {bit};"));
                if is_memory_op(instr.op_kind(0)) {
                    let addr = self.gen_addr(instr);
                    let addr = if instr.op_kind(1) == iced_x86::OpKind::Register {
                        // A register bit index is signed: the bit string can
                        // extend below the base address, so the element
                        // offset sign-extends the index and uses an
                        // arithmetic shift (SAR), not a logical one.
                        let shift = size.trailing_zeros();
                        let bytes = size / 8;
                        let sext = match reg_size(instr.op_register(1)) {
                            32 => format!("({bit_op} as i32)"),
                            _ => format!("({bit_op} as i16 as i32)"),
                        };
                        format!(
                            "{addr}.wrapping_add(({sext} >> {shift}).wrapping_mul({bytes}) as u32)"
                        )
                    } else {
                        addr
                    };
                    self.line(format!("let addr = {addr};"));
                    self.line(format!("let value = ctx.memory.read::<u{size}>(addr);"));
                    if operation == "bt" {
                        self.line("bt(value, bit, &mut ctx.cpu.flags);");
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
            // RDTSCP is RDTSC plus the TSC_AUX value in ECX; this machine
            // has a single synthetic core, so ECX reads 0.
            Rdtscp => {
                self.line("ctx.cpu.regs.set_edx_eax(rdtsc());");
                self.line("ctx.cpu.regs.ecx = 0;");
            }
            // SALC (undocumented D6): AL = CF ? 0xFF : 0x00.
            Salc => self.line(
                "ctx.cpu.regs.set_al(if ctx.cpu.flags.contains(Flags::CF) { 0xff } else { 0 });",
            ),
            // INT1/INT3 (the F1 and CC one-byte forms) dispatch through the
            // real-mode IVT like `int N`; flat code has no IDT to call.
            Int1 | Int3 => {
                let vector = if instr.mnemonic() == Int1 { 0x1 } else { 0x3 };
                if self.module.is_dos() {
                    self.line(format!(
                        "return dos::int(ctx, {:#x}, {vector:#x});",
                        instr.next_ip16()
                    ));
                } else {
                    self.line(format!(
                        "unhandled_interrupt({vector:#x}, {:#x});",
                        instr.ip32()
                    ));
                }
            }
            Pushf => self.line("ctx.push16(ctx.cpu.flags.bits() as u16 | 2);"),
            Popf => {
                self.line("ctx.cpu.flags = Flags::from_bits_truncate(ctx.pop16() as u32 & !2);")
            }
            Pushfd => self.line("ctx.push32(ctx.cpu.flags.bits() | 2);"),
            Popfd => self.line("ctx.cpu.flags = Flags::from_bits_truncate(ctx.pop32() & !2);"),
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
            // UD2: the guaranteed-#UD instruction; MSVC emits it for
            // unreachable paths.
            Ud2 => self.line(format!("unhandled_interrupt(0x6, {:#x});", instr.ip32())),
            // Prefetches, fences, PAUSE, and cache-line flushes are pure
            // hints on the emulated host.
            Prefetchnta | Prefetcht0 | Prefetcht1 | Prefetcht2 | Prefetch | Prefetchw
            | Prefetchwt1 | Sfence | Lfence | Mfence | Pause | Clflush | Clflushopt => {}
            // ENDBR is a plain NOP when control-flow enforcement is off.
            Endbr32 | Endbr64 => {}
            // No SSE unit is modeled, but the MXCSR value is real state.
            Stmxcsr => self.line(self.set_op(instr, 0, "ctx.cpu.mxcsr".into())),
            Ldmxcsr => self.line(format!("ctx.cpu.mxcsr = {};", self.get_op(instr, 0))),
            Hlt => {
                // Privileged in Windows usermode; on DOS it merely idles for
                // interrupts the model never delivers, so continue.
                if !self.module.is_dos() {
                    self.line(format!("unhandled_interrupt(0xd, {:#x});", instr.ip32()));
                }
            }
            Cmpxchg8b => {
                // Compare EDX:EAX with m64; on equal write ECX:EBX, else load
                // EDX:EAX from memory. Only ZF is affected.
                let addr = self.gen_addr(instr);
                self.line(format!(
                    "let cmpxchg8b_old = ctx.memory.read::<u64>({addr});"
                ));
                self.line("let cmpxchg8b_eq = ctx.cpu.regs.get_edx_eax() == cmpxchg8b_old;");
                self.line("ctx.cpu.flags.set(Flags::ZF, cmpxchg8b_eq);");
                self.line("if cmpxchg8b_eq {");
                self.line(format!(
                    "ctx.memory.write::<u64>({addr}, ((ctx.cpu.regs.ecx as u64) << 32) | (ctx.cpu.regs.ebx as u64));"
                ));
                self.line("} else {");
                self.line("ctx.cpu.regs.set_edx_eax(cmpxchg8b_old);");
                self.line("}");
            }
            Xgetbv => {
                self.line("let (xgetbv_eax, xgetbv_edx) = xgetbv(ctx.cpu.regs.ecx);");
                self.line("ctx.cpu.regs.eax = xgetbv_eax;");
                self.line("ctx.cpu.regs.edx = xgetbv_edx;");
            }
            Xadd => {
                assert_eq!(instr.op_count(), 2);
                let dst = self.get_op(instr, 0);
                let src = self.get_op(instr, 1);
                self.line("{");
                self.line(format!("let xadd_dst = {dst};"));
                self.line(format!(
                    "let xadd_tmp = add(xadd_dst, {src}, &mut ctx.cpu.flags);"
                ));
                // If the destination is memory its address may use the
                // source register, so write memory before the register
                // takes the old destination value (same as xchg).
                if is_memory_op(instr.op_kind(0)) {
                    self.line(self.set_op(instr, 0, "xadd_tmp".into()));
                    self.line(self.set_op(instr, 1, "xadd_dst".into()));
                } else {
                    self.line(self.set_op(instr, 1, "xadd_dst".into()));
                    self.line(self.set_op(instr, 0, "xadd_tmp".into()));
                }
                self.line("}");
            }
            Bsf | Bsr => {
                let func = instr_name(instr);
                self.line(format!(
                    "if let Some(res) = {func}({}, &mut ctx.cpu.flags) {{ {} }}",
                    self.get_op(instr, 1),
                    self.set_op(instr, 0, "res".into())
                ));
            }
            // CMOVcc shares its condition with the corresponding SETcc.
            Cmove | Cmovne | Cmovg | Cmovge | Cmovl | Cmovle | Cmova | Cmovae | Cmovb | Cmovbe
            | Cmovo | Cmovno | Cmovs | Cmovns | Cmovp | Cmovnp => {
                let cond = instr_name(instr).replacen("cmov", "set", 1);
                self.line(format!(
                    "if ctx.{}() != 0 {{ {} }}",
                    cond,
                    self.set_op(instr, 0, self.get_op(instr, 1))
                ));
            }
            // CBW/CWDE: sign extend to next larger ax
            Cbw => self.line("ctx.cpu.regs.set_ax(ctx.cpu.regs.get_al() as i8 as i16 as u16);"),
            Cwde => self.line("ctx.cpu.regs.eax = ctx.cpu.regs.get_ax() as i16 as i32 as u32;"),

            // CWD/CDQ: sign extend to dx:ax
            Cwd => self.line("ctx.cpu.regs.set_dx_ax(ctx.cpu.regs.get_ax() as i16 as i32 as u32);"),
            Cdq => self.line("ctx.cpu.regs.set_edx_eax(ctx.cpu.regs.eax as i32 as i64 as u64);"),

            Stc | Clc | Cmc | Std | Cld | Sahf | Lahf => {
                self.line(format!("{}(ctx);", instr_name(instr)));
            }

            Cli | Sti => {
                self.line(format!("ctx.{}();", instr_name(instr)));
            }
            Aaa => self.line("ctx.aaa();"),
            Aas => self.line("ctx.aas();"),
            Aad => {
                assert_eq!(instr.op_count(), 1);
                self.line(format!("ctx.aad({});", self.get_op(instr, 0)));
            }
            Aam => {
                assert_eq!(instr.op_count(), 1);
                let base = self.get_op(instr, 0);
                // AAM with a zero base raises #DE rather than dividing by
                // zero in the helper.
                let trap = self.divide_error(instr);
                self.line(format!("if {base} == 0 {{ {trap}; }}"));
                self.line(format!("ctx.aam({base});"));
            }
            Daa => self.line("ctx.daa();"),
            Das => self.line("ctx.das();"),

            In => {
                assert_eq!(instr.op_count(), 2);
                let port = if instr.op1_kind() == iced_x86::OpKind::Immediate8 {
                    format!("{:#x}u16", instr.immediate8())
                } else {
                    self.get_op(instr, 1)
                };
                let width = op_size(instr, 0);
                let value = if width == 32 {
                    format!("port_in({port}, {width})")
                } else {
                    format!("port_in({port}, {width}) as u{width}")
                };
                self.line(self.set_op(instr, 0, value));
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
                let value = self.get_op(instr, 1);
                let value = if width == 32 {
                    value
                } else {
                    format!("{value} as u32")
                };
                if self.module.is_dos() {
                    self.line(format!("dos::out(ctx, {port}, {value});"));
                } else {
                    self.line(format!("port_out({port}, {value}, {width});"));
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
                    self.line(format!(
                        "let ptr_offset = {};",
                        get_mem("u32".into(), address.clone())
                    ));
                    self.line(format!(
                        "let ptr_segment = {};",
                        get_mem("u16".into(), format!("{address}.wrapping_add(4u32)"))
                    ));
                    self.line(format!("ctx.cpu.regs.{segment} = ptr_segment;"));
                    self.line(self.set_op(instr, 0, "ptr_offset".into()));
                }
            }

            Xlatb => {
                // XLAT reads [seg:(E)BX + AL]: the base is BX or EBX per the
                // address-size attribute, and the segment may be overridden.
                let off = if instr.memory_base() == iced_x86::Register::BX {
                    // 16-bit addressing wraps the index sum to 16 bits.
                    "ctx.cpu.regs.get_bx().wrapping_add(ctx.cpu.regs.get_al() as u16) as u32"
                        .to_string()
                } else {
                    "ctx.cpu.regs.ebx.wrapping_add(ctx.cpu.regs.get_al() as u32)".to_string()
                };
                let addr = if self.module.segment_addressed() {
                    format!(
                        "segofs(ctx.cpu.regs.get_{}(), ({off}) as u16)",
                        reg_name(instr.memory_segment())
                    )
                } else if instr.memory_segment() == iced_x86::Register::FS {
                    format!("ctx.cpu.regs.fs_base.wrapping_add({off})")
                } else {
                    off
                };
                self.line(format!("ctx.xlat_addr({addr});"));
            }

            // ARPL compares and adjusts the RPL fields of two selectors; it
            // needs no descriptor-table model. iced reports a register
            // operand as the full 32-bit register, but only its low word
            // participates.
            Arpl => {
                self.line(format!(
                    "let arpl_dst = ({}) as u16;",
                    self.get_op(instr, 0)
                ));
                let write = match instr.op0_kind() {
                    iced_x86::OpKind::Register => {
                        let set = match instr.op_register(0) {
                            iced_x86::Register::EAX | iced_x86::Register::AX => "set_ax",
                            iced_x86::Register::ECX | iced_x86::Register::CX => "set_cx",
                            iced_x86::Register::EDX | iced_x86::Register::DX => "set_dx",
                            iced_x86::Register::EBX | iced_x86::Register::BX => "set_bx",
                            iced_x86::Register::ESP | iced_x86::Register::SP => "set_sp",
                            iced_x86::Register::EBP | iced_x86::Register::BP => "set_bp",
                            iced_x86::Register::ESI | iced_x86::Register::SI => "set_si",
                            iced_x86::Register::EDI | iced_x86::Register::DI => "set_di",
                            r => panic!("unhandled ARPL destination register: {r:?}"),
                        };
                        format!("ctx.cpu.regs.{set}(res);")
                    }
                    k if is_memory_op(k) => {
                        format!("ctx.memory.write::<u16>({}, res);", self.gen_addr(instr))
                    }
                    k => panic!("unhandled ARPL destination operand kind: {k:?}"),
                };
                self.line(format!(
                    "if let Some(res) = arpl(arpl_dst, ({}) as u16, &mut ctx.cpu.flags) {{ {write} }}",
                    self.get_op(instr, 1),
                ));
            }
            // This machine has no LDT or task and is not in protected mode,
            // so the system registers store as zero.
            Sldt | Str | Smsw => self.line(self.set_op(instr, 0, "0".into())),
            // SGDT/SIDT store the 6-byte descriptor-table register, which is
            // null on this machine.
            Sgdt | Sidt => {
                self.line(format!(
                    "ctx.{}({});",
                    instr_name(instr),
                    self.gen_addr(instr)
                ));
            }

            // BOUND checks an index against the inclusive [lower, upper]
            // bounds pair in memory and raises #BR when it is out of range.
            Bound => {
                let addr = self.gen_addr(instr);
                let (mem_t, cast, step) = if op_size(instr, 0) == 16 {
                    ("u16", "i16", 2)
                } else {
                    ("u32", "i32", 4)
                };
                self.line(format!(
                    "let bound_lo = ctx.memory.read::<{mem_t}>({addr}) as {cast} as i32;"
                ));
                self.line(format!("let bound_hi = ctx.memory.read::<{mem_t}>({addr}.wrapping_add({step}u32)) as {cast} as i32;"));
                self.line(format!(
                    "let bound_idx = ({}) as {cast} as i32;",
                    self.get_op(instr, 0)
                ));
                self.line(format!("if bound(bound_idx, bound_lo, bound_hi) {{ unhandled_interrupt(0x5, {:#x}); }}", instr.ip32()));
            }

            // The remaining system instructions are privileged in Windows
            // usermode, and on DOS they would need protected-mode machinery
            // the machine cannot model. Emit an explicit #GP trap rather than
            // silently dropping them.
            Lar | Lsl | Verr | Verw | Lldt | Ltr | Lmsw | Lgdt | Lidt | Invlpg | Invd | Wbinvd
            | Clts | Rdmsr | Wrmsr | Rdpmc | Rsm | Monitor | Mwait | Sysenter | Sysexit
            | Swapgs | Xsetbv => {
                self.line(format!("unhandled_interrupt(0xd, {:#x});", instr.ip32()));
            }

            // Everything else here is an instruction this CPU does not
            // implement at all — extension ISAs like 3DNow! and PadLock,
            // SVM/VMX virtualization, user interrupts, CET state tracking,
            // and newer state-save or RNG opcodes. Emit an explicit #UD trap
            // so translation continues past the instruction instead of
            // dropping the rest of the block.
            Ud0 | Ud1 | Xstore | Xcryptcbc | Xcryptcfb | Xcryptctr | Xcryptecb | Xcryptofb
            | Montmul | Xsha1 | Xsha256 | Getsec | Loadall | Jmpe | Xsave | Xrstor | Xsaveopt
            | Xsaves | Xrstors | Xsavec | Rdrand | Rdseed | Rdpid | Clzero | Clwb | Pcommit
            | Wbnoinvd | Monitorx | Mwaitx | Tpause | Umonitor | Umwait | Cldemote | Rdpkru
            | Wrpkru | Vmgexit | Vmrun | Vmmcall | Vmload | Vmsave | Skinit | Stgi | Clgi
            | Invlpga | Invlpgb | Enqcmd | Enqcmds | Movdiri | Movdir64b | Serialize | Hreset
            | Clui | Stui | Testui | Uiret | Senduipi | Setssbsy | Clrssbsy | Incsspd | Incsspq
            | Rstorssp | Saveprevssp | Wrssd | Wrussd | Wrssq | Wrussq => {
                self.line(format!("unhandled_interrupt(0x6, {:#x});", instr.ip32()));
            }

            _ => return false,
        }
        true
    }
}
