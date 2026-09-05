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
        match instr.mnemonic() {
            Movsb | Movsw | Movsd | // x
            Lodsb | Lodsw | Lodsd | // x
            Insb | Insw | Insd | // x
            Outsb | Outsw | Outsd | // x
            Stosb | Stosw | Stosd => {
                let name = instr_name(instr);
                // Note: repe/repne behaves the same as rep for these instructions,
                if instr.has_rep_prefix() || instr.has_repne_prefix() {
                    self.line(format!("ctx.rep(Rep::REP, Context::{name});"));
                } else {
                    self.line(format!("ctx.{name}();"));
                }
            }

            // Careful: cmps/scas use repe, not rep
            Cmpsb | Cmpsw | Cmpsd | //x
            Scasb | Scasw | Scasd => {
                let name = instr_name(instr);
                if instr.has_repe_prefix() {
                    self.line(format!("ctx.rep(Rep::REPE, Context::{name});"));
                } else if instr.has_repne_prefix() {
                    self.line(format!("ctx.rep(Rep::REPNE, Context::{name});"));
                } else {
                    self.line(format!("ctx.{name}();"));
                };
            }

            _ => return false,
        }
        true
    }
}
