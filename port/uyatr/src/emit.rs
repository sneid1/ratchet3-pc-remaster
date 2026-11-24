//! Rust for one function of the game's code.
//!
//! A function becomes `pub extern "C-unwind" fn fXXXXXXXX(c: &mut Cpu, mut l: u32)` (`uycpu::Fn`: the runtime's table calls it,
//! from the executable or a level's library, and a stop unwinds through it): a loop over its blocks, entered at the label `l`
//! (its entry, or any label the runtime's table gives for it: a jump table's case, a code address held in data). Each
//! instruction becomes the Rust that does what the R5900 does with it; the branch delay slots run where the hardware runs them;
//! `jal` and `jalr` call through the runtime's table of translated code (`Cpu::call`), and a call that came back anywhere but
//! the instruction after its delay slot returns too (`Cpu::came_back`: the code after it already ran in the frame that went
//! there); `jr $ra` returns (leaving the address in `Cpu::ret`); a jump out of the function is a tail call. A loop that only
//! polls memory or hardware the game itself does not change in it (a wait for the vertical blank, a DMA, another thread) calls
//! `Cpu::spin` on its way back, where the runtime lets time and the other threads go on.

use crate::decode::{decode, decode_vu, I, K, M, V};
use std::collections::BTreeSet;
use std::fmt::Write;

pub struct Func {
    pub start: u32,
    pub end: u32,
    pub insns: Vec<I>,
    /// The word after the function: the delay slot of a branch that ends it.
    pub tail: Option<I>,
    /// Block starts: the entry, branch targets inside the function, labels (from data, other programs, address pairs). The
    /// instruction after a call or a branch is not one of its own: the code runs on into it in the same block (a return
    /// address is never entered from outside: the runtime's table holds labels only).
    pub leaders: BTreeSet<u32>,
}

impl Func {
    fn at(&self, pc: u32) -> I {
        if pc == self.end {
            return self.tail.expect("checked by func: a branch at the end has its delay slot after it");
        }
        self.insns[((pc - self.start) / 4) as usize]
    }
    fn inside(&self, a: u32) -> bool {
        a >= self.start && a < self.end
    }
}

pub fn func(start: u32, end: u32, words: &[u32], next: Option<u32>, labels: &BTreeSet<u32>) -> Result<Func, String> {
    let insns: Vec<I> = words.iter().map(|&w| decode(w)).collect();
    let tail = next.map(decode);
    let mut leaders = BTreeSet::new();
    leaders.insert(start);
    for (k, i) in insns.iter().enumerate() {
        let pc = start + 4 * k as u32;
        if i.m == M::Unknown {
            return Err(format!("{pc:08x}: unknown instruction {:08x} in function {start:08x}", i.w));
        }
        if i.m == M::Vu && decode_vu(i.w) == V::Unknown {
            return Err(format!("{pc:08x}: unknown VU0 macro instruction {:08x} in function {start:08x}", i.w));
        }
        if i.has_delay() {
            let d = pc + 4;
            // The delay slot is the next word, in this function or (the last instruction's) the one after it.
            let slot = if d < end { Some(insns[k + 1]) } else { tail };
            let Some(slot) = slot else {
                return Err(format!("{pc:08x}: the branch's delay slot is past the end of the program's code"));
            };
            if slot.has_delay() {
                return Err(format!("{pc:08x}: a branch in a branch's delay slot"));
            }
            if slot.m == M::Unknown {
                return Err(format!("{pc:08x}: the delay slot {:08x} is not an instruction", slot.w));
            }
            if i.is_cond_branch() || i.m == M::J {
                let t = if i.m == M::J { i.jump_target(pc) } else { i.branch_target(pc) };
                if t >= start && t < end && !i.links() {
                    if t % 4 != 0 {
                        return Err(format!("{pc:08x}: branch to unaligned {t:08x}"));
                    }
                    leaders.insert(t);
                }
            }
        }
    }
    for &a in labels.range(start..end) {
        leaders.insert(a);
    }
    Ok(Func { start, end, insns, tail, leaders })
}

fn r(n: usize) -> String {
    if n == 0 { "0u64".into() } else { format!("c.r[{n}]") }
}
fn w(n: usize) -> String {
    if n == 0 { "0u32".into() } else { format!("(c.r[{n}] as u32)") }
}
fn rh(n: usize) -> String {
    if n == 0 { "0u64".into() } else { format!("c.rh[{n}]") }
}
fn set(n: usize, e: &str) -> String {
    if n == 0 { String::new() } else { format!("c.r[{n}] = {e};") }
}
fn sx(e: &str) -> String {
    format!("(({e}) as i32 as i64 as u64)")
}
fn addr(i: &I) -> String {
    let off = i.simm() as u32;
    if i.rs() == 0 {
        format!("0x{off:08x}u32")
    } else if off == 0 {
        w(i.rs())
    } else {
        format!("{}.wrapping_add(0x{off:08x})", w(i.rs()))
    }
}

/// The 128-bit register pair as (lo, hi) for the MMI helpers.
fn q(n: usize) -> String {
    format!("({}, {})", r(n), rh(n))
}
fn setq(n: usize, e: &str) -> String {
    if n == 0 { format!("let _ = {e};") } else { format!("{{ let (a, b) = {e}; c.r[{n}] = a; c.rh[{n}] = b; }}") }
}

pub struct Ctx {
    /// Spin loops found: (pc of the back branch) for the report.
    pub spins: Vec<u32>,
    /// The function's registers that hold its return address (`ra_copies`).
    pub ra_copies: [bool; 32],
    /// The registers no instruction of the function writes (`unwritten`).
    pub unwritten: [bool; 32],
    /// The loads of the view's horizontal tangent that make the view (main.rs, `VIEW_LOADS`): each reads it times the host's
    /// `view_k` (`Cpu::widen`).
    pub widen: std::collections::BTreeSet<u32>,
    /// The loads of a view tangent read times the field of view's factor (main.rs, `FOV_LOADS`; `Cpu::fov`).
    pub fov: std::collections::BTreeSet<u32>,
    /// The loads of the game's camera speed option read times the camera speed setting's factor (main.rs, `CAMERA_SPEED`;
    /// `Cpu::cam_speed`).
    pub cam_speed: std::collections::BTreeSet<u32>,
    /// The hero's and its camera's reads of the camera option (main.rs, `CAMERA_SETTING`; `Cpu::camera_setting`).
    pub cam_setting: std::collections::BTreeSet<u32>,
    /// The moby cull's drawn byte (main.rs, `DRAWN`): the branch on the byte as the cull read it, to the store that sets it.
    pub drawn: std::collections::BTreeMap<u32, u32>,
    /// The UI's screen draw (main.rs, `UI_SCREEN`): its entry places the HUD's groups (`Cpu::ui_screen`).
    pub ui_screen: std::collections::BTreeSet<u32>,
    /// The loads of a UI element's x (main.rs, `UI_LOADS`): each reads it plus the element's shift (`Cpu::ui_shift`).
    pub ui_loads: std::collections::BTreeSet<u32>,
    /// The 2D drawers' screen test (main.rs, `UI_EDGES`): the frame's right edge (`ori`: true) and the left edge's test (`slti`)
    /// moved to the wide screen's (`Cpu::ui_margin`).
    pub ui_edges: std::collections::BTreeMap<u32, bool>,
}

/// The registers no instruction of the function writes. A `jr` through one goes where its caller pointed it: a return through
/// a link register other than ra (level 4's 0x4b0d70 calls the VU0 entries at 0x4b0e2c with `jalr t1, t0`, and they return with
/// `jr t1`), or a tail call through a register the caller set.
fn unwritten(f: &Func) -> [bool; 32] {
    let mut w = [false; 32];
    for i in f.insns.iter().chain(f.tail.iter()) {
        for d in dests(i) {
            w[d] = true;
        }
    }
    std::array::from_fn(|r| r != 0 && !w[r])
}

/// The registers whose every write in the function copies ra (level 1's 0x40cac0: `addi t7, ra, 0`, two calls, `jr t7`): a
/// jump through one returns.
fn ra_copies(f: &Func) -> [bool; 32] {
    use M::*;
    let (mut copy, mut other) = ([false; 32], [false; 32]);
    for i in f.insns.iter().chain(f.tail.iter()) {
        let from_ra = match i.m {
            Addi | Addiu | Daddi | Daddiu => i.rs() == 31 && i.simm() == 0,
            Addu | Daddu | Or => (i.rs() == 31 && i.rt() == 0) || (i.rs() == 0 && i.rt() == 31),
            _ => false,
        };
        for d in dests(i) {
            if from_ra { copy[d] = true } else { other[d] = true }
        }
    }
    std::array::from_fn(|r| r != 0 && r != 31 && copy[r] && !other[r])
}

/// The function's Rust.
pub fn emit(f: &Func, ctx: &mut Ctx, out: &mut String) -> Result<(), String> {
    ctx.ra_copies = ra_copies(f);
    ctx.unwritten = unwritten(f);
    writeln!(out, "#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]").unwrap();
    writeln!(out, "pub extern \"C-unwind\" fn f{:08x}(c: &mut Cpu, mut l: u32) {{", f.start).unwrap();
    writeln!(out, "loop {{ match l {{").unwrap();
    for &lead in &f.leaders {
        writeln!(out, "0x{lead:08x} => {{").unwrap();
        let mut pc = lead;
        loop {
            if pc != lead && f.leaders.contains(&pc) {
                writeln!(out, "l = 0x{pc:08x}; continue; }}").unwrap();
                break;
            }
            if pc >= f.end {
                // Off the function's end: the code there goes on (a 4-byte piece the bounds split off, a fall-through).
                writeln!(out, "c.call(0x{pc:08x}, 0x{pc:08x}); return; }}").unwrap();
                break;
            }
            let i = f.at(pc);
            if i.has_delay() {
                let d = f.at(pc + 4);
                if control(f, ctx, &i, pc, &d, out)? {
                    // A call, or a branch not taken: on with the instruction after the delay slot.
                    pc += 8;
                    continue;
                }
                writeln!(out, "}}").unwrap();
                break;
            }
            insn_at(&i, pc, ctx, out)?;
            pc += 4;
        }
    }
    writeln!(out, "_ => c.bad_label(0x{:08x}, l), }} }} }}", f.start).unwrap();
    Ok(())
}

/// The way to the block at `t` from the instruction at `pc`: inside the function a jump to its block, else a tail call.
fn goto(f: &Func, t: u32, pc: u32) -> String {
    if f.inside(t) && f.leaders.contains(&t) {
        format!("l = 0x{t:08x}; continue;")
    } else {
        format!("c.call(0x{t:08x}, 0x{pc:08x}); return;")
    }
}

/// A backward branch inside the function closes a loop: the runtime's stall check (`Cpu::back`).
fn back(f: &Func, t: u32, pc: u32) -> String {
    if f.inside(t) && t <= pc { format!("c.back(0x{pc:08x}); ") } else { String::new() }
}

fn call(t: &str, pc: u32) -> String {
    format!("c.call({t}, 0x{pc:08x}); if c.ret != 0x{:08x} {{ c.came_back(0x{pc:08x}); return; }}", pc + 8)
}

/// A backward branch at `pc` to `t` closing a loop that only polls: no store, no call, and every load's base register is not
/// written in the loop (it reads the same place every time), with at least one load or hardware poll in it.
fn is_spin(f: &Func, t: u32, pc: u32) -> bool {
    if !f.inside(t) || t > pc || pc + 8 - t > 4 * 16 {
        return false;
    }
    let mut written = [false; 32];
    let mut polls = false;
    let mut bases = Vec::new();
    for a in (t..=pc + 4).step_by(4) {
        let i = f.at(a);
        if i.is_store() || matches!(i.m, M::Jal | M::Jalr | M::Syscall | M::Jr | M::Break | M::Mtc0 | M::Ctc2 | M::Qmtc2 | M::Vu | M::Ctc1) {
            return false;
        }
        // A conditional branch inside is the loop's exit test (libkernel's vertical-blank wait 0x11f390 tests I_STAT, then a
        // flag); anything else that moves control (a jump, a call) is not a poll.
        if a != pc && i.has_delay() && (!i.is_cond_branch() || i.links()) {
            return false;
        }
        if i.is_load() {
            polls = true;
            bases.push(i.rs());
        }
        if matches!(i.m, M::Mfc0 | M::Cfc2 | M::Bc0f | M::Bc0t | M::Bc2f | M::Bc2t) {
            polls = true;
        }
        for d in dests(&i) {
            written[d] = true;
        }
    }
    polls && bases.iter().all(|&b| !written[b])
}

/// The GPRs an instruction writes (for the spin test).
fn dests(i: &I) -> Vec<usize> {
    use M::*;
    match i.m {
        Sll | Srl | Sra | Sllv | Srlv | Srav | Movz | Movn | Mfhi | Mflo | Dsllv | Dsrlv | Dsrav | Add | Addu | Sub | Subu | And | Or
        | Xor | Nor | Mfsa | Slt | Sltu | Dadd | Daddu | Dsub | Dsubu | Dsll | Dsrl | Dsra | Dsll32 | Dsrl32 | Dsra32 | Mult | Multu
        | Jalr | Mfhi1 | Mflo1 | Mult1 | Multu1 | Madd | Maddu | Madd1 | Maddu1 => vec![i.rd()],
        Addi | Addiu | Slti | Sltiu | Andi | Ori | Xori | Lui | Daddi | Daddiu | Lb | Lh | Lw | Lbu | Lhu | Lwu | Ld | Lq | Lwl | Lwr
        | Ldl | Ldr | Mfc0 | Mfc1 | Cfc1 | Cfc2 | Qmfc2 => vec![i.rt()],
        Bltzal | Bgezal | Bltzall | Bgezall | Jal => vec![31],
        _ => if (i.w >> 26) == 0x1c { vec![i.rd()] } else { vec![] },
    }
}

fn cond(i: &I) -> String {
    use M::*;
    let (a, b) = (format!("({} as i64)", r(i.rs())), format!("({} as i64)", r(i.rt())));
    match i.m {
        Beq | Beql => format!("{} == {}", r(i.rs()), r(i.rt())),
        Bne | Bnel => format!("{} != {}", r(i.rs()), r(i.rt())),
        Blez | Blezl => format!("{a} <= 0"),
        Bgtz | Bgtzl => format!("{a} > 0"),
        Bltz | Bltzl | Bltzal | Bltzall => format!("{a} < 0"),
        Bgez | Bgezl | Bgezal | Bgezall => format!("{a} >= 0"),
        Bc1f | Bc1fl => "c.fcr31 & 0x0080_0000 == 0".into(),
        Bc1t | Bc1tl => "c.fcr31 & 0x0080_0000 != 0".into(),
        Bc0f | Bc0fl => "!c.cpcond0()".into(),
        Bc0t | Bc0tl => "c.cpcond0()".into(),
        Bc2f | Bc2fl => "!c.vu0_busy()".into(),
        Bc2t | Bc2tl => "c.vu0_busy()".into(),
        _ => {
            let _ = b;
            unreachable!("not a conditional branch: {:?}", i.m)
        }
    }
}

/// A jump, call or branch with its delay slot; true when control can go on to the instruction after the delay slot (a call
/// returns there, a conditional branch not taken goes there), which the caller emits next in the same block.
fn control(f: &Func, ctx: &mut Ctx, i: &I, pc: u32, d: &I, out: &mut String) -> Result<bool, String> {
    if let Some(&store) = ctx.drawn.get(&pc) {
        // `Cpu::drawn_side`: the byte as the original view leaves it, then the cull's own branch and store.
        let s = f.at(store);
        if i.m != M::Bne || i.rt() != 0 || s.m != M::Sb {
            return Err(format!("{pc:08x}: the moby cull's drawn byte: a {:?} and a {:?} at {store:08x}, not a bne on it and its sb", i.m, s.m));
        }
        writeln!(out, "c.drawn_side({}, {});", addr(&s), r(i.rs())).unwrap();
    }
    let mut delay = String::new();
    insn_at(d, pc + 4, ctx, &mut delay)?;
    let next = pc + 8;
    match i.m {
        M::J => {
            let t = i.jump_target(pc);
            let spin = if is_spin(f, t, pc) {
                ctx.spins.push(pc);
                format!("c.spin(0x{pc:08x}); ")
            } else {
                back(f, t, pc)
            };
            writeln!(out, "{delay} {spin}{}", goto(f, t, pc)).unwrap();
            Ok(false)
        }
        M::Jal => {
            let t = i.jump_target(pc);
            writeln!(out, "c.r[31] = 0x{next:08x}; {delay} {}", call(&format!("0x{t:08x}"), pc)).unwrap();
            Ok(true)
        }
        M::Jr => {
            if i.rs() == 31 || ctx.ra_copies[i.rs()] {
                writeln!(out, "let t = {}; {delay} c.ret = t; return;", w(i.rs())).unwrap();
            } else {
                let cases: Vec<String> = f.leaders.iter().map(|a| format!("0x{a:08x}")).collect();
                // Through a register the function never writes: to translated code if the table has it (a tail call), else a
                // return to it (where the call came back to is checked by the caller: a frame that does not go on there returns too).
                let out_of = if ctx.unwritten[i.rs()] {
                    format!("if c.lookup(t).is_some() {{ c.call(t, 0x{pc:08x}); }} else {{ c.ret = t; }} return;")
                } else {
                    format!("c.call(t, 0x{pc:08x}); return;")
                };
                writeln!(out, "let t = {}; {delay} match t {{ {} => {{ l = t; continue; }} _ => {{ {out_of} }} }}", w(i.rs()), cases.join(" | ")).unwrap();
            }
            Ok(false)
        }
        M::Jalr => {
            let link = set(i.rd(), &format!("0x{next:08x}"));
            writeln!(out, "let t = {}; {link} {delay} {}", w(i.rs()), call("t", pc)).unwrap();
            Ok(true)
        }
        _ => {
            let t = i.branch_target(pc);
            let likely = i.is_likely();
            let taken = if i.links() {
                call(&format!("0x{t:08x}"), pc)
            } else {
                let spin = if is_spin(f, t, pc) {
                    ctx.spins.push(pc);
                    format!("c.spin(0x{pc:08x}); ")
                } else {
                    back(f, t, pc)
                };
                format!("{spin}{}", goto(f, t, pc))
            };
            let link = if i.links() { format!("c.r[31] = 0x{next:08x};") } else { String::new() };
            if likely {
                writeln!(out, "let t = {}; {link} if t {{ {delay} {taken} }}", cond(i)).unwrap();
            } else {
                writeln!(out, "let t = {}; {link} {delay} if t {{ {taken} }}", cond(i)).unwrap();
            }
            Ok(true)
        }
    }
}

/// An instruction; or (`Ctx::widen`) a load of the view's horizontal tangent read times the host's `view_k` and `fov` (the
/// tangent is the loaded word, or a quadword's first, whose second, the vertical tangent, is read times `fov`); or (`Ctx::fov`)
/// a load of a tangent read times `fov`; or (`Ctx::drawn`) the moby cull's store of its drawn byte, made only when the original
/// view passes the moby too (`Cpu::in_original_view`).
fn insn_at(i: &I, pc: u32, ctx: &Ctx, out: &mut String) -> Result<(), String> {
    if ctx.ui_screen.contains(&pc) {
        writeln!(out, "c.ui_screen({});", w(4)).unwrap();
    }
    if let Some(&bound) = ctx.ui_edges.get(&pc) {
        let (rt, rs, imm) = (i.rt(), i.rs(), i.simm() as i64);
        let s = match (bound, i.m) {
            (true, M::Ori) if rs == 0 && rt != 0 => format!("c.r[{rt}] = ({}i64 + c.ui_margin()) as u64;", i.uimm()),
            (false, M::Slti) if rt != 0 => format!("c.r[{rt}] = (({} as i64) < {imm} - c.ui_margin()) as u64;", r(rs)),
            (_, m) => return Err(format!("{pc:08x}: a 2D drawer's screen test is a {m:?}")),
        };
        writeln!(out, "{s}").unwrap();
        return Ok(());
    }
    if ctx.ui_loads.contains(&pc) {
        if i.m != M::Lwc1 || i.simm() != 0x10 {
            return Err(format!("{pc:08x}: a UI element's x load is a {:?} at {:#x}", i.m, i.simm()));
        }
        let rt = i.rt();
        writeln!(out, "{{ let x = c.rd32({}); c.f[{rt}] = match c.ui_shift({}) {{ Some(s) => (f32::from_bits(x) + s).to_bits(), None => x }}; }}", addr(i), w(i.rs())).unwrap();
        return Ok(());
    }
    if ctx.drawn.values().any(|&s| s == pc) {
        let mut store = String::new();
        insn(i, pc, &mut store)?;
        writeln!(out, "if c.in_original_view() {{ {} }}", store.trim_end()).unwrap();
        return Ok(());
    }
    if ctx.cam_setting.contains(&pc) {
        if i.m != M::Lbu {
            return Err(format!("{pc:08x}: a read of the camera option is a {:?}", i.m));
        }
        let rt = i.rt();
        if rt != 0 {
            writeln!(out, "{{ let a = {}; let v = c.rd8(a); c.r[{rt}] = c.camera_setting(a, v) as u64; }}", addr(i)).unwrap();
        }
        return Ok(());
    }
    if ctx.cam_speed.contains(&pc) {
        if i.m != M::Lwc1 {
            return Err(format!("{pc:08x}: a load of the camera speed option is a {:?}", i.m));
        }
        writeln!(out, "{{ let w = c.rd32({}); c.f[{}] = c.cam_speed(w); }}", addr(i), i.rt()).unwrap();
        return Ok(());
    }
    if ctx.fov.contains(&pc) {
        if i.m != M::Lwc1 {
            return Err(format!("{pc:08x}: a view tangent load to scale by the field of view is a {:?}", i.m));
        }
        writeln!(out, "{{ let w = c.rd32({}); c.f[{}] = c.fov(w); }}", addr(i), i.rt()).unwrap();
        return Ok(());
    }
    if !ctx.widen.contains(&pc) {
        return insn(i, pc, out);
    }
    let rt = i.rt();
    // A quadword (h, v, +0xB8, +0xBC): h widened, v times the field of view's factor.
    let s = match i.m {
        M::Lwc1 => format!("{{ let w = c.rd32({}); c.f[{rt}] = c.widen(w); }}", addr(i)),
        M::Lq => setq(rt, &format!("{{ let (a, b) = c.rd128({}); let h = c.widen(a as u32); let v = c.fov((a >> 32) as u32); ((v as u64) << 32 | h as u64, b) }}", addr(i))),
        M::Lqc2 if rt != 0 => format!("{{ let (a, b) = c.rd128({}); let h = c.widen(a as u32); let v = c.fov((a >> 32) as u32); c.vf[{rt}] = [h, v, b as u32, (b >> 32) as u32]; }}", addr(i)),
        m => return Err(format!("{pc:08x}: a view tangent load to widen is a {m:?}")),
    };
    writeln!(out, "{s}").unwrap();
    Ok(())
}

/// One instruction that is not a branch or jump.
pub fn insn(i: &I, pc: u32, out: &mut String) -> Result<(), String> {
    use M::*;
    let (rs, rt, rd, sa) = (i.rs(), i.rt(), i.rd(), i.sa());
    let simm = i.simm();
    let uimm = i.uimm();
    let s = match i.m {
        Sll => {
            if i.w == 0 {
                return Ok(());
            }
            set(rd, &sx(&format!("{} << {sa}", w(rt))))
        }
        Srl => set(rd, &sx(&format!("{} >> {sa}", w(rt)))),
        Sra => set(rd, &sx(&format!("(({} as i32) >> {sa}) as u32", w(rt)))),
        Sllv => set(rd, &sx(&format!("{}.wrapping_shl({} & 31)", w(rt), w(rs)))),
        Srlv => set(rd, &sx(&format!("{}.wrapping_shr({} & 31)", w(rt), w(rs)))),
        Srav => set(rd, &sx(&format!("(({} as i32).wrapping_shr({} & 31)) as u32", w(rt), w(rs)))),
        Movz => if rd == 0 { String::new() } else { format!("if {} == 0 {{ c.r[{rd}] = {}; }}", r(rt), r(rs)) },
        Movn => if rd == 0 { String::new() } else { format!("if {} != 0 {{ c.r[{rd}] = {}; }}", r(rt), r(rs)) },
        Syscall => format!("c.syscall(0x{pc:08x});"),
        Break => format!("c.brk(0x{pc:08x}, 0x{:05x});", (i.w >> 6) & 0xfffff),
        Sync => String::new(),
        Mfhi => set(rd, "c.hi"),
        Mflo => set(rd, "c.lo"),
        Mthi => format!("c.hi = {};", r(rs)),
        Mtlo => format!("c.lo = {};", r(rs)),
        Mfhi1 => set(rd, "c.hi1"),
        Mflo1 => set(rd, "c.lo1"),
        Mthi1 => format!("c.hi1 = {};", r(rs)),
        Mtlo1 => format!("c.lo1 = {};", r(rs)),
        Dsllv => set(rd, &format!("{}.wrapping_shl(({} & 63) as u32)", r(rt), r(rs))),
        Dsrlv => set(rd, &format!("{}.wrapping_shr(({} & 63) as u32)", r(rt), r(rs))),
        Dsrav => set(rd, &format!("(({} as i64).wrapping_shr(({} & 63) as u32)) as u64", r(rt), r(rs))),
        Mult | Mult1 | Multu | Multu1 | Madd | Madd1 | Maddu | Maddu1 => {
            let one = matches!(i.m, Mult1 | Multu1 | Madd1 | Maddu1);
            let (lo, hi) = if one { ("c.lo1", "c.hi1") } else { ("c.lo", "c.hi") };
            let p = if matches!(i.m, Mult | Mult1 | Madd | Madd1) {
                format!("(({} as i32 as i64) * ({} as i32 as i64)) as u64", w(rs), w(rt))
            } else {
                format!("({} as u64) * ({} as u64)", w(rs), w(rt))
            };
            let acc = if matches!(i.m, Madd | Madd1 | Maddu | Maddu1) { format!(".wrapping_add((({hi} as u32 as u64) << 32) | ({lo} as u32 as u64))") } else { String::new() };
            format!("{{ let p: u64 = ({p}){acc}; {lo} = p as u32 as i32 as i64 as u64; {hi} = (p >> 32) as u32 as i32 as i64 as u64; {} }}", set(rd, lo))
        }
        Div | Div1 => {
            let (lo, hi) = if i.m == Div1 { ("c.lo1", "c.hi1") } else { ("c.lo", "c.hi") };
            format!("{{ let (a, b) = ({} as i32, {} as i32); let (q, m) = uycpu::div32(a, b); {lo} = q as i64 as u64; {hi} = m as i64 as u64; }}", w(rs), w(rt))
        }
        Divu | Divu1 => {
            let (lo, hi) = if i.m == Divu1 { ("c.lo1", "c.hi1") } else { ("c.lo", "c.hi") };
            format!("{{ let (a, b) = ({}, {}); let (q, m) = uycpu::divu32(a, b); {lo} = q as i32 as i64 as u64; {hi} = m as i32 as i64 as u64; }}", w(rs), w(rt))
        }
        Add | Addu => set(rd, &sx(&format!("{}.wrapping_add({})", w(rs), w(rt)))),
        Sub | Subu => set(rd, &sx(&format!("{}.wrapping_sub({})", w(rs), w(rt)))),
        And => set(rd, &format!("{} & {}", r(rs), r(rt))),
        Or => set(rd, &format!("{} | {}", r(rs), r(rt))),
        Xor => set(rd, &format!("{} ^ {}", r(rs), r(rt))),
        Nor => set(rd, &format!("!({} | {})", r(rs), r(rt))),
        Mfsa => set(rd, "c.sa as u64"),
        Mtsa => format!("c.sa = {};", w(rs)),
        Mtsab => format!("c.sa = (({} ^ 0x{:x}) & 15) * 8;", w(rs), uimm & 15),
        Mtsah => format!("c.sa = (({} ^ 0x{:x}) & 7) * 16;", w(rs), uimm & 7),
        Slt => set(rd, &format!("(({} as i64) < ({} as i64)) as u64", r(rs), r(rt))),
        Sltu => set(rd, &format!("({} < {}) as u64", r(rs), r(rt))),
        Dadd | Daddu => set(rd, &format!("{}.wrapping_add({})", r(rs), r(rt))),
        Dsub | Dsubu => set(rd, &format!("{}.wrapping_sub({})", r(rs), r(rt))),
        Tge | Tgeu | Tlt | Tltu | Teq | Tne => {
            let c = match i.m {
                Tge => format!("({} as i64) >= ({} as i64)", r(rs), r(rt)),
                Tgeu => format!("{} >= {}", r(rs), r(rt)),
                Tlt => format!("({} as i64) < ({} as i64)", r(rs), r(rt)),
                Tltu => format!("{} < {}", r(rs), r(rt)),
                Teq => format!("{} == {}", r(rs), r(rt)),
                _ => format!("{} != {}", r(rs), r(rt)),
            };
            format!("if {c} {{ c.trap(0x{pc:08x}); }}")
        }
        Tgei | Tgeiu | Tlti | Tltiu | Teqi | Tnei => {
            let im = simm as i64;
            let c = match i.m {
                Tgei => format!("({} as i64) >= {im}", r(rs)),
                Tgeiu => format!("{} >= {}u64", r(rs), im as u64),
                Tlti => format!("({} as i64) < {im}", r(rs)),
                Tltiu => format!("{} < {}u64", r(rs), im as u64),
                Teqi => format!("({} as i64) == {im}", r(rs)),
                _ => format!("({} as i64) != {im}", r(rs)),
            };
            format!("if {c} {{ c.trap(0x{pc:08x}); }}")
        }
        Dsll => set(rd, &format!("{} << {sa}", r(rt))),
        Dsrl => set(rd, &format!("{} >> {sa}", r(rt))),
        Dsra => set(rd, &format!("(({} as i64) >> {sa}) as u64", r(rt))),
        Dsll32 => set(rd, &format!("{} << {}", r(rt), sa + 32)),
        Dsrl32 => set(rd, &format!("{} >> {}", r(rt), sa + 32)),
        Dsra32 => set(rd, &format!("(({} as i64) >> {}) as u64", r(rt), sa + 32)),
        Addi | Addiu => set(rt, &sx(&format!("{}.wrapping_add(0x{:08x})", w(rs), simm as u32))),
        Daddi | Daddiu => set(rt, &format!("{}.wrapping_add(0x{:016x})", r(rs), simm as i64 as u64)),
        Slti => set(rt, &format!("(({} as i64) < {}) as u64", r(rs), simm)),
        Sltiu => set(rt, &format!("({} < 0x{:016x}) as u64", r(rs), simm as i64 as u64)),
        Andi => set(rt, &format!("{} & 0x{uimm:x}", r(rs))),
        Ori => set(rt, &format!("{} | 0x{uimm:x}", r(rs))),
        Xori => set(rt, &format!("{} ^ 0x{uimm:x}", r(rs))),
        Lui => set(rt, &format!("0x{:016x}", ((uimm << 16) as i32) as i64 as u64)),
        Lb => ld(rt, &format!("c.rd8({}) as i8 as i64 as u64", addr(i))),
        Lbu => ld(rt, &format!("c.rd8({}) as u64", addr(i))),
        Lh => ld(rt, &format!("c.rd16({}) as i16 as i64 as u64", addr(i))),
        Lhu => ld(rt, &format!("c.rd16({}) as u64", addr(i))),
        Lw => ld(rt, &format!("c.rd32({}) as i32 as i64 as u64", addr(i))),
        Lwu => ld(rt, &format!("c.rd32({}) as u64", addr(i))),
        Ld => ld(rt, &format!("c.rd64({})", addr(i))),
        Lq => setq(rt, &format!("c.rd128({})", addr(i))),
        Lwl => ld(rt, &format!("c.lwl({}, {})", addr(i), r(rt))),
        Lwr => ld(rt, &format!("c.lwr({}, {})", addr(i), r(rt))),
        Ldl => ld(rt, &format!("c.ldl({}, {})", addr(i), r(rt))),
        Ldr => ld(rt, &format!("c.ldr({}, {})", addr(i), r(rt))),
        Sb => format!("c.wr8({}, {} as u8);", addr(i), r(rt)),
        Sh => format!("c.wr16({}, {} as u16);", addr(i), r(rt)),
        Sw => format!("c.wr32({}, {} as u32);", addr(i), r(rt)),
        Sd => format!("c.wr64({}, {});", addr(i), r(rt)),
        Sq => format!("c.wr128({}, {}, {});", addr(i), r(rt), rh(rt)),
        Swl => format!("c.swl({}, {});", addr(i), r(rt)),
        Swr => format!("c.swr({}, {});", addr(i), r(rt)),
        Sdl => format!("c.sdl({}, {});", addr(i), r(rt)),
        Sdr => format!("c.sdr({}, {});", addr(i), r(rt)),
        Cache | Pref => String::new(),
        Lwc1 => format!("c.f[{rt}] = c.rd32({});", addr(i)),
        Swc1 => format!("c.wr32({}, c.f[{rt}]);", addr(i)),
        Lqc2 => {
            if rt == 0 {
                format!("let _ = c.rd128({});", addr(i))
            } else {
                format!("{{ let (a, b) = c.rd128({}); c.vf[{rt}] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }}", addr(i))
            }
        }
        Sqc2 => format!("{{ let v = c.vf[{rt}]; c.wr128({}, v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }}", addr(i)),
        Mfc0 => set(rt, &format!("c.mfc0({rd}, 0x{:x}) as i32 as i64 as u64", i.w & 0x7ff)),
        Mtc0 => format!("c.mtc0({rd}, 0x{:x}, {});", i.w & 0x7ff, w(rt)),
        Ei => "c.ei();".into(),
        Di => "c.di();".into(),
        // ERET: the program goes on at EPC (ErrorEPC under ERL), which the code set itself (libkernel's kernel-mode store puts
        // $ra there, so it is a return).
        Eret => format!("{{ let t = c.eret(); if t == c.r[31] as u32 {{ c.ret = t; return; }} c.call(t, 0x{pc:08x}); return; }}"),
        Tlbr | Tlbwi | Tlbwr | Tlbp => format!("c.kernel_only(0x{pc:08x}, \"{:?}\");", i.m),
        Mfc1 => set(rt, &format!("c.f[{rd}] as i32 as i64 as u64")),
        Mtc1 => format!("c.f[{rd}] = {};", w(rt)),
        Cfc1 => set(rt, &format!("c.cfc1({rd}) as i32 as i64 as u64")),
        Ctc1 => format!("c.ctc1({rd}, {});", w(rt)),
        AddS | SubS | MulS | DivS | SqrtS | AbsS | MovS | NegS | RsqrtS | AddaS | SubaS | MulaS | MaddS | MsubS | MaddaS | MsubaS
        | CvtWS | MaxS | MinS | CF | CEq | CLt | CLe | CvtSW => fpu(i),
        Qmfc2 => setq(rt, &format!("{{ let v = c.vf[{rd}]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }}")),
        Qmtc2 => {
            if rd == 0 {
                String::new()
            } else {
                format!("c.vf[{rd}] = [{} as u32, ({} >> 32) as u32, {} as u32, ({} >> 32) as u32];", r(rt), r(rt), rh(rt), rh(rt))
            }
        }
        Cfc2 => set(rt, &format!("c.cfc2({rd}) as i32 as i64 as u64")),
        Ctc2 => format!("c.ctc2({rd}, {}, 0x{pc:08x});", w(rt)),
        Vu => vu(i, pc)?,
        Plzcw => set(rd, &format!("uycpu::mmi::plzcw({})", r(rs))),
        Pmfhl => match sa {
            0 => setq(rd, "uycpu::mmi::pmfhl_lw(c.lo, c.lo1, c.hi, c.hi1)"),
            1 => setq(rd, "uycpu::mmi::pmfhl_uw(c.lo, c.lo1, c.hi, c.hi1)"),
            2 => setq(rd, "uycpu::mmi::pmfhl_slw(c.lo, c.lo1, c.hi, c.hi1)"),
            3 => setq(rd, "uycpu::mmi::pmfhl_lh(c.lo, c.lo1, c.hi, c.hi1)"),
            4 => setq(rd, "uycpu::mmi::pmfhl_sh(c.lo, c.lo1, c.hi, c.hi1)"),
            _ => return Err(format!("{pc:08x}: PMFHL with fmt {sa}")),
        },
        Pmthl => {
            if sa != 0 {
                return Err(format!("{pc:08x}: PMTHL with fmt {sa}"));
            }
            format!("{{ let (a, b) = {}; c.lo = (c.lo & !0xffff_ffff) | (a & 0xffff_ffff); c.hi = (c.hi & !0xffff_ffff) | (a >> 32); c.lo1 = (c.lo1 & !0xffff_ffff) | (b & 0xffff_ffff); c.hi1 = (c.hi1 & !0xffff_ffff) | (b >> 32); }}", q(rs))
        }
        Pmfhi => setq(rd, "(c.hi, c.hi1)"),
        Pmflo => setq(rd, "(c.lo, c.lo1)"),
        Pmthi => format!("{{ let (a, b) = {}; c.hi = a; c.hi1 = b; }}", q(rs)),
        Pmtlo => format!("{{ let (a, b) = {}; c.lo = a; c.lo1 = b; }}", q(rs)),
        Psllh | Psrlh | Psrah | Psllw | Psrlw | Psraw => {
            let name = format!("{:?}", i.m).to_lowercase();
            setq(rd, &format!("uycpu::mmi::{name}({}, {sa})", q(rt)))
        }
        Qfsrv => setq(rd, &format!("uycpu::mmi::qfsrv({}, {}, c.sa)", q(rs), q(rt))),
        Pmaddw | Pmsubw | Pmultw | Pmadduw | Pmultuw | Pmaddh | Pmsubh | Phmadh | Phmsbh | Pmulth => {
            let name = format!("{:?}", i.m).to_lowercase();
            setq(rd, &format!("uycpu::mmi::{name}(c, {}, {})", q(rs), q(rt)))
        }
        // The divisions write HI and LO only.
        Pdivw | Pdivuw | Pdivbw => {
            let name = format!("{:?}", i.m).to_lowercase();
            format!("let _ = uycpu::mmi::{name}(c, {}, {});", q(rs), q(rt))
        }
        Paddw | Psubw | Pcgtw | Pmaxw | Paddh | Psubh | Pcgth | Pmaxh | Paddb | Psubb | Pcgtb | Paddsw | Psubsw | Pextlw | Ppacw | Paddsh
        | Psubsh | Pextlh | Ppach | Paddsb | Psubsb | Pextlb | Ppacb | Pceqw | Pminw | Padsbh | Pceqh | Pminh | Pceqb | Padduw
        | Psubuw | Pextuw | Padduh | Psubuh | Pextuh | Paddub | Psubub | Pextub | Psllvw | Psrlvw | Pinth | Pcpyld | Pand | Pxor
        | Psravw | Pinteh | Pcpyud | Por | Pnor => {
            let name = format!("{:?}", i.m).to_lowercase();
            setq(rd, &format!("uycpu::mmi::{name}({}, {})", q(rs), q(rt)))
        }
        Pext5 | Ppac5 | Pabsw | Pabsh | Pexeh | Prevh | Pexew | Prot3w | Pexch | Pcpyh | Pexcw => {
            let name = format!("{:?}", i.m).to_lowercase();
            setq(rd, &format!("uycpu::mmi::{name}({})", q(rt)))
        }
        _ => return Err(format!("{pc:08x}: {:?} is not a plain instruction", i.m)),
    };
    if !s.is_empty() {
        writeln!(out, "{s}").unwrap();
    }
    Ok(())
}

/// A load into `rt`: the access is made even when `rt` is $zero (a hardware register read can matter).
fn ld(rt: usize, e: &str) -> String {
    if rt == 0 { format!("let _ = {e};") } else { format!("c.r[{rt}] = {e};") }
}

fn fpu(i: &I) -> String {
    use M::*;
    let ft = i.rt();
    let fs = i.rd();
    let fd = i.sa() as usize;
    match i.m {
        AddS => format!("c.f[{fd}] = uycpu::fpu::add(c.f[{fs}], c.f[{ft}]);"),
        SubS => format!("c.f[{fd}] = uycpu::fpu::sub(c.f[{fs}], c.f[{ft}]);"),
        MulS => format!("c.f[{fd}] = uycpu::fpu::mul(c.f[{fs}], c.f[{ft}]);"),
        DivS => format!("c.f[{fd}] = uycpu::fpu::div(c, c.f[{fs}], c.f[{ft}]);"),
        SqrtS => format!("c.f[{fd}] = uycpu::fpu::sqrt(c, c.f[{ft}]);"),
        RsqrtS => format!("c.f[{fd}] = uycpu::fpu::rsqrt(c, c.f[{fs}], c.f[{ft}]);"),
        AbsS => format!("c.f[{fd}] = c.f[{fs}] & 0x7fff_ffff;"),
        NegS => format!("c.f[{fd}] = c.f[{fs}] ^ 0x8000_0000;"),
        MovS => format!("c.f[{fd}] = c.f[{fs}];"),
        AddaS => format!("c.facc = uycpu::fpu::add(c.f[{fs}], c.f[{ft}]);"),
        SubaS => format!("c.facc = uycpu::fpu::sub(c.f[{fs}], c.f[{ft}]);"),
        MulaS => format!("c.facc = uycpu::fpu::mul(c.f[{fs}], c.f[{ft}]);"),
        MaddS => format!("c.f[{fd}] = uycpu::fpu::add(c.facc, uycpu::fpu::mul(c.f[{fs}], c.f[{ft}]));"),
        MsubS => format!("c.f[{fd}] = uycpu::fpu::sub(c.facc, uycpu::fpu::mul(c.f[{fs}], c.f[{ft}]));"),
        MaddaS => format!("c.facc = uycpu::fpu::add(c.facc, uycpu::fpu::mul(c.f[{fs}], c.f[{ft}]));"),
        MsubaS => format!("c.facc = uycpu::fpu::sub(c.facc, uycpu::fpu::mul(c.f[{fs}], c.f[{ft}]));"),
        MaxS => format!("c.f[{fd}] = uycpu::fpu::max(c.f[{fs}], c.f[{ft}]);"),
        MinS => format!("c.f[{fd}] = uycpu::fpu::min(c.f[{fs}], c.f[{ft}]);"),
        CvtWS => format!("c.f[{fd}] = uycpu::fpu::cvt_w(c.f[{fs}]);"),
        CvtSW => format!("c.f[{fd}] = uycpu::fpu::cvt_s(c.f[{fs}]);"),
        CF => "c.fcr31 &= !0x0080_0000;".into(),
        CEq => format!("c.set_fc(uycpu::fpu::eq(c.f[{fs}], c.f[{ft}]));"),
        CLt => format!("c.set_fc(uycpu::fpu::lt(c.f[{fs}], c.f[{ft}]));"),
        CLe => format!("c.set_fc(uycpu::fpu::le(c.f[{fs}], c.f[{ft}]));"),
        _ => unreachable!(),
    }
}

/// A VU0 macro instruction: the upper operations on VF and ACC with the dest lanes `d` (bit 3 x .. bit 0 w), the integer ones on
/// VI, VCALLMS.
fn vu(i: &I, pc: u32) -> Result<String, String> {
    let d = (i.w >> 21) & 15;
    let ft = i.rt();
    let fs = i.rd();
    let fd = i.sa() as usize;
    let kn = |k: K| match k {
        K::Add => 0,
        K::Sub => 1,
        K::Madd => 2,
        K::Msub => 3,
        K::Max => 4,
        K::Mini => 5,
        K::Mul => 6,
    };
    // Writes into vf0 are dropped (it reads 0, 0, 0, 1 always); ACC is index 32.
    let dst = |acc: bool| if acc { 32usize } else { fd };
    Ok(match decode_vu(i.w) {
        V::Bc { k, acc, bc } => format!("uycpu::vu0::fmac::<{}, {d}>(c, {}, {fs}, uycpu::vu0::bcast(c.vf[{ft}][{bc}]));", kn(k), dst(acc)),
        V::Qi { k, acc, i: isi } => format!("uycpu::vu0::fmac::<{}, {d}>(c, {}, {fs}, uycpu::vu0::bcast(c.vi[{}]));", kn(k), dst(acc), if isi { 21 } else { 22 }),
        V::Full { k, acc } => format!("uycpu::vu0::fmac::<{}, {d}>(c, {}, {fs}, c.vf[{ft}]);", kn(k), dst(acc)),
        V::Opmula => format!("uycpu::vu0::opmula(c, {fs}, {ft});"),
        V::Opmsub => format!("uycpu::vu0::opmsub(c, {fd}, {fs}, {ft});"),
        V::Itof(n) => format!("uycpu::vu0::itof::<{n}, {d}>(c, {ft}, {fs});"),
        V::Ftoi(n) => format!("uycpu::vu0::ftoi::<{n}, {d}>(c, {ft}, {fs});"),
        V::Abs => format!("uycpu::vu0::abs::<{d}>(c, {ft}, {fs});"),
        V::Clip => format!("uycpu::vu0::clip(c, {fs}, {ft});"),
        V::Nop => String::new(),
        V::Move => format!("uycpu::vu0::mov::<{d}>(c, {ft}, c.vf[{fs}]);"),
        V::Mr32 => format!("{{ let v = c.vf[{fs}]; uycpu::vu0::mov::<{d}>(c, {ft}, [v[1], v[2], v[3], v[0]]); }}"),
        V::Lqi => format!("uycpu::vu0::lqi::<{d}>(c, {ft}, {fs});"),
        V::Sqi => format!("uycpu::vu0::sqi::<{d}>(c, {fs}, {ft});"),
        V::Lqd => format!("uycpu::vu0::lqd::<{d}>(c, {ft}, {fs});"),
        V::Sqd => format!("uycpu::vu0::sqd::<{d}>(c, {fs}, {ft});"),
        V::Div | V::Sqrt | V::Rsqrt => {
            let (fsf, ftf) = ((i.w >> 21) & 3, (i.w >> 23) & 3);
            let k = match decode_vu(i.w) {
                V::Div => 0,
                V::Sqrt => 1,
                _ => 2,
            };
            format!("uycpu::vu0::qop::<{k}>(c, c.vf[{fs}][{fsf}], c.vf[{ft}][{ftf}]);")
        }
        V::Waitq => String::new(),
        V::Mtir => {
            let fsf = (i.w >> 21) & 3;
            format!("c.set_vi({ft}, c.vf[{fs}][{fsf}] & 0xffff);")
        }
        V::Mfir => format!("{{ let v = c.vi[{fs}] as u16 as i16 as i32 as u32; uycpu::vu0::mov::<{d}>(c, {ft}, [v; 4]); }}"),
        V::Ilwr => format!("uycpu::vu0::ilwr::<{d}>(c, {ft}, {fs});"),
        V::Iswr => format!("uycpu::vu0::iswr::<{d}>(c, {ft}, {fs});"),
        V::Rnext => format!("uycpu::vu0::rnext::<{d}>(c, {ft});"),
        V::Rget => format!("uycpu::vu0::rget::<{d}>(c, {ft});"),
        V::Rinit => format!("uycpu::vu0::rinit(c, c.vf[{fs}][{}]);", (i.w >> 21) & 3),
        V::Rxor => format!("uycpu::vu0::rxor(c, c.vf[{fs}][{}]);", (i.w >> 21) & 3),
        V::Iadd => format!("c.set_vi({fd}, c.vi[{fs}].wrapping_add(c.vi[{ft}]));"),
        V::Isub => format!("c.set_vi({fd}, c.vi[{fs}].wrapping_sub(c.vi[{ft}]));"),
        V::Iaddi => format!("c.set_vi({ft}, c.vi[{fs}].wrapping_add(0x{:08x}));", ((((i.w >> 6) & 31) as i32) << 27 >> 27) as u32),
        V::Iand => format!("c.set_vi({fd}, c.vi[{fs}] & c.vi[{ft}]);"),
        V::Ior => format!("c.set_vi({fd}, c.vi[{fs}] | c.vi[{ft}]);"),
        V::Callms => format!("c.vcallms(0x{:x}, 0x{pc:08x});", ((i.w >> 6) & 0x7fff) * 8),
        V::Callmsr => format!("c.vcallms(c.vi[27] * 8, 0x{pc:08x});"),
        V::Unknown => return Err(format!("{pc:08x}: unknown VU0 macro instruction {:08x}", i.w)),
    })
}
