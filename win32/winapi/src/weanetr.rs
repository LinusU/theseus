use runtime::{Cont, Context};

fn thiscall_return(ctx: &mut Context, stack_args: u16) -> Cont {
    let return_addr = ctx.memory.read::<u32>(ctx.cpu.regs.esp);
    ctx.cpu.regs.esp += 4 + (stack_args as u32) * 4;
    ctx.cpu.regs.eax = 0;
    ctx.indirect(return_addr)
}

fn thiscall_return_with_value(ctx: &mut Context, stack_args: u16, value: u32) -> Cont {
    let return_addr = ctx.memory.read::<u32>(ctx.cpu.regs.esp);
    ctx.cpu.regs.esp += 4 + (stack_args as u32) * 4;
    ctx.cpu.regs.eax = value;
    ctx.indirect(return_addr)
}

// The Populous executable links against the weanetr.dll MLDPlay class using
// __thiscall, where the 'this' pointer is passed in ECX and any stack
// arguments are callee-cleaned. The following thunks mirror the correct
// stack cleanup for each arg-count used by the MLDPlay methods.

pub fn thunk_0_stdcall(ctx: &mut Context) -> Cont {
    thiscall_return(ctx, 0)
}

pub fn thunk_1_stdcall(ctx: &mut Context) -> Cont {
    thiscall_return(ctx, 1)
}

pub fn thunk_2_stdcall(ctx: &mut Context) -> Cont {
    thiscall_return(ctx, 2)
}

pub fn thunk_3_stdcall(ctx: &mut Context) -> Cont {
    thiscall_return(ctx, 3)
}

pub fn thunk_4_stdcall(ctx: &mut Context) -> Cont {
    thiscall_return(ctx, 4)
}

pub fn thunk_5_stdcall(ctx: &mut Context) -> Cont {
    thiscall_return(ctx, 5)
}

pub fn thunk_6_stdcall(ctx: &mut Context) -> Cont {
    thiscall_return(ctx, 6)
}

pub fn thunk_7_stdcall(ctx: &mut Context) -> Cont {
    thiscall_return(ctx, 7)
}

pub fn thunk_8_stdcall(ctx: &mut Context) -> Cont {
    thiscall_return(ctx, 8)
}

// The few MLDPlay methods Populous actually uses during startup.

pub fn StartupNetwork(ctx: &mut Context) -> Cont {
    // Report success so the game proceeds to the lobby/service enumeration.
    thiscall_return_with_value(ctx, 1, 1)
}

pub fn AreWeLobbied(ctx: &mut Context) -> Cont {
    // Return 0 (not lobbied); the game then calls EnumerateServices.
    thiscall_return_with_value(ctx, 8, 0)
}

pub fn EnumerateServices(ctx: &mut Context) -> Cont {
    let return_addr = ctx.memory.read::<u32>(ctx.cpu.regs.esp);
    let callback = ctx.memory.read::<u32>(ctx.cpu.regs.esp + 4);
    let user_context = ctx.memory.read::<u32>(ctx.cpu.regs.esp + 8);

    // Populous's registration callback (0x4e3d80) expects:
    //   - a table at 0x96d3a4 indexed by [0x5a7f7c]+1
    //   - each entry to point at an object whose +0x18 is a wide-char name
    // The object and info can be the same block; use a stable address inside
    // the data region the game already clears.
    let service = 0x96d400u32;
    let name = 0x96d500u32;
    ctx.memory.write::<u16>(name, 0);
    ctx.memory.write::<u32>(service + 0x10, 0);
    ctx.memory.write::<u32>(service + 0x18, name);
    ctx.memory.write::<u32>(service + 0x24, 0);

    // Pre-fill the next few info-table slots so the callback can read
    // 0x96d3a4[count+1] without deref'ing null. Index 1 overlaps 0x96d3a8[0]
    // so the same value also serves as the registered object pointer.
    let count = ctx.memory.read::<u32>(0x5a7f7cu32);
    for i in 0..4 {
        ctx.memory
            .write::<u32>(0x96d3a4u32 + (count + i) * 4, service);
    }

    // The game passes a __stdcall callback that pops 5 dwords. Call it
    // synchronously so it can update the service table before we return.
    let cont = ctx.indirect(callback);
    ctx.call32_x86(cont, vec![service, 0, 0, 0, user_context]);

    ctx.cpu.regs.eax = 1;
    ctx.cpu.regs.esp += 4 + 2 * 4;
    ctx.indirect(return_addr)
}
