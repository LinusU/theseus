use crate::codegen::{CodeGen, instr_name, memory_kind_base};

impl<'a> CodeGen<'a> {
    pub fn codegen_string(&mut self, instr: &iced_x86::Instruction) -> bool {
        use iced_x86::Mnemonic::*;
        // The `movsd` and `cmpsd` mnemonics are shared between string
        // instructions and SSE2 instructions. String forms use the implicit
        // `DS:(E)SI`/`ES:(E)DI` memory operand kinds (`MemorySeg*`/`MemoryES*`).
        if (instr.mnemonic() == Movsd || instr.mnemonic() == Cmpsd)
            && memory_kind_base(instr.op0_kind()).is_none()
        {
            return false;
        }
        // A 16-bit address-size attribute (`67` in 32-bit code, plain forms
        // in 16-bit code) shows up as the 16-bit implicit operand kinds;
        // those forms count in CX and advance SI/DI rather than ECX/ESI/EDI.
        let addr16 = (0..instr.op_count()).any(|i| {
            matches!(
                instr.op_kind(i),
                iced_x86::OpKind::MemorySegSI
                    | iced_x86::OpKind::MemorySegDI
                    | iced_x86::OpKind::MemoryESDI
            )
        });
        let suffix = if addr16 { "_16" } else { "" };
        let rep_fn = if addr16 { "rep16" } else { "rep" };
        match instr.mnemonic() {
            Movsb | Movsw | Movsd | // x
            Lodsb | Lodsw | Lodsd | // x
            Insb | Insw | Insd | // x
            Outsb | Outsw | Outsd | // x
            Stosb | Stosw | Stosd => {
                let name = instr_name(instr);
                // Note: repe/repne behaves the same as rep for these instructions,
                if instr.has_rep_prefix() || instr.has_repne_prefix() {
                    self.line(format!("ctx.{rep_fn}(Rep::REP, Context::{name}{suffix});"));
                } else {
                    self.line(format!("ctx.{name}{suffix}();"));
                }
            }

            // Careful: cmps/scas use repe, not rep
            Cmpsb | Cmpsw | Cmpsd | //x
            Scasb | Scasw | Scasd => {
                let name = instr_name(instr);
                if instr.has_repe_prefix() {
                    self.line(format!("ctx.{rep_fn}(Rep::REPE, Context::{name}{suffix});"));
                } else if instr.has_repne_prefix() {
                    self.line(format!("ctx.{rep_fn}(Rep::REPNE, Context::{name}{suffix});"));
                } else {
                    self.line(format!("ctx.{name}{suffix}();"));
                };
            }

            _ => return false,
        }
        true
    }
}
