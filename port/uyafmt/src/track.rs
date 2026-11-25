//! The tracker: one line per structure kind, written to docs/decode.md by `uyafmt disc`.
//!
//! Every sector of the disc is claimed by a file or listed as unassigned. Every byte of a file belongs to a structure instance
//! (a node with a line) or to its file kind's `unknown bytes` line. A decoder accounts for a structure's bytes by marking them as
//! it reads them as named fields (or as padding it requires to be zero); bytes it does not mark stay `unread`. A line is
//! `mapped` when a decoder walked every instance of it on the disc without error, left no byte unread, and the line names the
//! game function its fields come from. Mapped says where a structure is and who reads it, not that its content is understood.
//! A mapped line is also `loaded` when its decoder returns the structure's data and the port has used that data (drawn,
//! played or shown it): `LOADED` lists those lines, and a line goes on it only in the commit whose check looked at the result.
//! A mapped line's bytes are `claimed` rather than proven when its marks rest on the partial evidence PLAN.md lists under
//! "Unproven claims in the map" (`CLAIMED`).
//!
//! Bytes the game never reads are left out by the player's decision: a decoder that has shown nothing reads some bytes of a
//! structure skips them (`Node::skip`; they no longer count as unread), and a whole structure nothing reads goes on an ignored
//! line (`Tracker::ignored_line`), counted apart like the excluded files. "No reader found yet" is not that: the line's
//! decoder or game string says what was searched.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Space {
    /// Bytes as they lie on the disc.
    Disc,
    /// Bytes of a block unpacked from an LZ stream (counted in unpacked bytes).
    Unpacked,
}

pub type LineId = usize;
pub type InstId = usize;

/// What reads a line's structure in the game; `NOTHING` for bytes no code reads (padding the decoder requires to be zero, or a
/// value the disc's builder wrote that the decoder checks against what it must be).
pub const NOTHING: &str = "nothing (checked by the decoder)";

pub struct Line {
    pub group: String,
    pub name: String,
    pub decoder: &'static str,
    pub game: &'static str,
    pub kind: Kind,
    pub holders: BTreeSet<String>,
    pub instances: u64,
    pub bytes: u64,
    pub space: Space,
    pub unread: u64,
    /// Bytes the decoder skipped because nothing reads them (`Node::skip`).
    pub skipped: u64,
    pub errors: Vec<String>,
    pub n_errors: u64,
    pub note: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Data,
    /// A program image: counted apart, `recompiled` or `not recompiled`.
    Code { recompiled: bool },
    Unknown,
    Unassigned,
    /// Left out by the player's decision (multiplayer): claimed so no sector is unassigned, counted apart, never decoded.
    Excluded,
    /// A structure nothing in the game reads (the player's decision: never-read bytes are ignored); counted apart.
    Ignored,
}

struct Inst {
    line: Option<LineId>,
    space: Space,
    size: u64,
    kids: Vec<InstId>,
}

/// A sorted set of byte ranges.
#[derive(Default, Clone)]
pub struct Ranges(Vec<(u64, u64)>);

impl Ranges {
    pub fn add(&mut self, a: u64, b: u64) {
        if a >= b {
            return;
        }
        let v = &mut self.0;
        // Fast path: appending at or after the last range.
        if let Some(last) = v.last_mut() {
            if a >= last.0 {
                if a <= last.1 {
                    last.1 = last.1.max(b);
                    return;
                }
                v.push((a, b));
                return;
            }
        } else {
            v.push((a, b));
            return;
        }
        let i = v.partition_point(|r| r.1 < a);
        let mut j = i;
        let (mut na, mut nb) = (a, b);
        while j < v.len() && v[j].0 <= nb {
            na = na.min(v[j].0);
            nb = nb.max(v[j].1);
            j += 1;
        }
        v.splice(i..j, [(na, nb)]);
    }

    /// The parts of [a, b) not in the set.
    pub fn missing(&self, a: u64, b: u64) -> Vec<(u64, u64)> {
        let mut out = Vec::new();
        let mut at = a;
        for &(x, y) in &self.0 {
            if y <= at || x >= b {
                continue;
            }
            if x > at {
                out.push((at, x));
            }
            at = at.max(y);
        }
        if at < b {
            out.push((at, b));
        }
        out
    }

    pub fn total(&self) -> u64 {
        self.0.iter().map(|r| r.1 - r.0).sum()
    }

    /// The gaps of [0, size) not in the set.
    pub fn holes(&self, size: u64) -> Vec<(u64, u64)> {
        let mut out = Vec::new();
        let mut at = 0;
        for &(a, b) in &self.0 {
            if a > at {
                out.push((at, a.min(size)));
            }
            at = at.max(b);
        }
        if at < size {
            out.push((at, size));
        }
        out
    }

    pub fn union(&self, other: &Ranges) -> Ranges {
        let mut r = self.clone();
        for &(a, b) in &other.0 {
            r.add(a, b);
        }
        r
    }
}

/// An open structure instance (or a file's root, which has no line): the bytes its decoder has read, and its children.
pub struct Node {
    pub id: InstId,
    pub line: Option<LineId>,
    pub size: u64,
    pub space: Space,
    pub holder: String,
    /// Group that the root's unknown bytes are listed under.
    pub group: String,
    /// Where it is, for messages: e.g. "L01 LEVEL +0x1234".
    pub place: String,
    read: Ranges,
    kids: Ranges,
    skipped: Ranges,
    error: Option<String>,
}

impl Node {
    /// Skips `len` bytes at `at`: bytes the decoder has shown nothing in the game reads (the line's game string says why).
    pub fn skip(&mut self, at: u64, len: u64) {
        self.skipped.add(at, at + len);
    }

    /// Marks `len` bytes at `at` as read by the decoder as named fields.
    pub fn mark(&mut self, at: u64, len: u64) {
        self.read.add(at, at + len);
    }

    /// Records the decoder's error: the line cannot be decoded.
    pub fn fail(&mut self, e: String) {
        if self.error.is_none() {
            self.error = Some(e);
        }
    }

    pub fn failed(&self) -> bool {
        self.error.is_some()
    }

    /// The byte ranges neither read nor skipped by the decoder nor held by a child.
    pub fn holes(&self) -> Vec<(u64, u64)> {
        self.read.union(&self.kids).union(&self.skipped).holes(self.size)
    }

    /// Whether [at, at + len) touches a child already placed.
    pub fn overlaps(&self, at: u64, len: u64) -> bool {
        self.kids.0.iter().any(|&(a, b)| a < at + len && at < b)
    }
}

pub struct Tracker {
    pub lines: Vec<Line>,
    index: HashMap<(String, String), LineId>,
    insts: Vec<Inst>,
    claims: Vec<(u64, u64, String)>,
    unknown_index: HashMap<String, LineId>,
}

/// Lines whose decoder returns their data and whose data the port has used: (group, line name prefix).
const LOADED: &[(&str, &str)] = &[
    // The core's LZ stream: the port unpacks it for the terrain (port/uya/src/native.rs).
    ("LEVEL level data", "core: LZ stream"),
    // The terrain as drawn natively (terrain::terrain; Veldin, field 2400, captures/terrain1), the same shapes in the same place as
    // the game drawing only its terrain programs (captures/terrainonly2). Its colour words are not loaded: the game relights them.
    ("LEVEL core terrain (unpacked)", "terrain header"),
    ("LEVEL core terrain (unpacked)", "terrain chunk table"),
    ("LEVEL core terrain (unpacked)", "terrain chunk VIF codes"),
    ("LEVEL core terrain (unpacked)", "terrain chunk GS register records"),
    ("LEVEL core terrain (unpacked)", "terrain chunk VU1 block header"),
    ("LEVEL core terrain (unpacked)", "terrain chunk vertex positions"),
    ("LEVEL core terrain (unpacked)", "terrain chunk base UV records"),
    ("LEVEL core terrain (unpacked)", "terrain chunk morph and extra UV records"),
    ("LEVEL core terrain (unpacked)", "terrain chunk strip vertex lists"),
    ("LEVEL core terrain (unpacked)", "terrain chunk morph lists"),
    ("LEVEL core terrain (unpacked)", "terrain chunk strip table"),
    ("LEVEL core terrain (unpacked)", "terrain chunk box corners"),
    // The terrain's textures (core::texture).
    ("LEVEL core index", "terrain texture records"),
    ("LEVEL core texture data (unpacked)", "terrain texture texels"),
    // The terrain's lit colours (terrain::light from the light table and the gameplay block's light sets): all 45,775 of Veldin's
    // equal the game's in RAM at field 2400 (`uyafmt colours`), drawn by the port's renderer (captures/native7-terrain).
    ("LEVEL core terrain (unpacked)", "terrain chunk light table"),
    ("LEVEL", "gameplay block: LZ stream"),
    ("LEVEL gameplay (unpacked)", "+0x04 table: 0x40-byte light-set records"),
];

/// Lines whose read or skipped marks rest on partial evidence (PLAN.md "Unproven claims in the map"): (decoder prefix, line name
/// substring, "" for every line of the decoder). Their bytes count as `claimed` until the step that loads the structure proves
/// them.
const CLAIMED: &[(&str, &str)] = &[
    ("gameplay::blob", "class variables"),
    ("gameplay::blob", "camera"),
    ("gameplay::blob", "+0x0C record"),
    ("gameplay::records_90", ""),
    ("tie::", ""),
    // The bytes skipped as read by nothing (header +2, +0xE, +0x10; base record +4; parts of the strip table entries): searched in
    // VU1 programs 1 and 7 and by mutation, not in every EE program.
    ("terrain::terrain", "VU1 block header"),
    ("terrain::terrain", "base UV records"),
    ("terrain::terrain", "strip table"),
    ("terrain::region3", ""),
    ("headers::wad_header", "WAD8"),
    ("frontwad::", ""),
    ("wad38::", ""),
    ("moby::", "packet B"),
    ("moby::", "collision"),
    ("anim::", ""),
    ("scene::", "object"),
    ("coremisc::index_tables", "word 0x1C"),
];

pub struct Totals {
    pub data_lines: usize,
    pub mapped_lines: usize,
    pub loaded_lines: usize,
    pub proven_bytes: u64,
    pub claimed_bytes: u64,
    pub unknown_bytes: u64,
    pub unassigned_sectors: u64,
    pub code_bytes: u64,
    pub excluded_bytes: u64,
    pub ignored_bytes: u64,
    pub overlays_recompiled: usize,
    pub overlays: usize,
}

impl Tracker {
    pub fn new() -> Tracker {
        Tracker { lines: Vec::new(), index: HashMap::new(), insts: Vec::new(), claims: Vec::new(), unknown_index: HashMap::new() }
    }

    /// The line for a structure kind, created on first use. `decoder` is the `module::function` that reads it ("" for none yet),
    /// `game` the R&C3 function(s) its fields come from ("" when not yet found).
    pub fn line(&mut self, group: &str, name: &str, decoder: &'static str, game: &'static str, space: Space) -> LineId {
        self.line_of(group, name, decoder, game, space, Kind::Data)
    }

    /// A line for a structure nothing in the game reads: `why` says what was searched (it is shown as the line's game function).
    pub fn ignored_line(&mut self, group: &str, name: &str, why: &'static str, space: Space) -> LineId {
        self.line_of(group, name, "", why, space, Kind::Ignored)
    }

    pub fn code_line(&mut self, group: &str, name: &str, recompiled: bool) -> LineId {
        self.line_of(group, name, "", "", Space::Disc, Kind::Code { recompiled })
    }

    /// A code line whose instances sit in a block unpacked from an LZ stream.
    pub fn unpacked_code_line(&mut self, group: &str, name: &str, recompiled: bool) -> LineId {
        self.line_of(group, name, "", "", Space::Unpacked, Kind::Code { recompiled })
    }

    fn line_of(&mut self, group: &str, name: &str, decoder: &'static str, game: &'static str, space: Space, kind: Kind) -> LineId {
        let key = (group.to_string(), name.to_string());
        if let Some(&id) = self.index.get(&key) {
            let l = &self.lines[id];
            assert!(l.decoder == decoder && l.game == game && l.space == space, "line {group} / {name} registered twice with different decoders");
            return id;
        }
        let id = self.lines.len();
        self.lines.push(Line {
            group: group.to_string(),
            name: name.to_string(),
            decoder,
            game,
            kind,
            holders: BTreeSet::new(),
            instances: 0,
            bytes: 0,
            space,
            unread: 0,
            skipped: 0,
            errors: Vec::new(),
            n_errors: 0,
            note: String::new(),
        });
        self.index.insert(key, id);
        id
    }

    /// Adds a note to a line (what is known of it, what is missing).
    pub fn note(&mut self, id: LineId, note: &str) {
        let n = &mut self.lines[id].note;
        if !n.contains(note) {
            if !n.is_empty() {
                n.push_str("; ");
            }
            n.push_str(note);
        }
    }

    /// Claims `count` sectors from `sector` for a file or a disc structure.
    pub fn claim(&mut self, sector: u64, count: u64, what: &str) {
        self.claims.push((sector, sector + count, what.to_string()));
    }

    /// Sectors the player left out of the decode: claimed, and listed on one excluded line.
    pub fn excluded(&mut self, name: &str, holder: &str, sector: u64, count: u64) {
        self.claim(sector, count, &format!("excluded {name} {holder}"));
        let id = self.line_of("Excluded by the player", name, "", "", Space::Disc, Kind::Excluded);
        let l = &mut self.lines[id];
        l.instances += 1;
        l.bytes += count * 0x800;
        l.holders.insert(holder.to_string());
    }

    /// A file's root node: `size` bytes, no line of its own; its unread bytes are listed under `group`'s unknown bytes.
    pub fn root(&mut self, group: &str, holder: &str, size: u64, space: Space) -> Node {
        let id = self.insts.len();
        self.insts.push(Inst { line: None, space, size, kids: Vec::new() });
        Node { id, line: None, size, space, holder: holder.to_string(), group: group.to_string(), place: format!("{holder} {group}"), read: Ranges::default(), skipped: Ranges::default(), kids: Ranges::default(), error: None }
    }

    /// A structure instance of `line`, `size` bytes at `at` in `parent`. A child in another space (an unpacked block inside an LZ
    /// stream) takes no bytes of its parent.
    pub fn child(&mut self, parent: &mut Node, line: LineId, at: u64, size: u64) -> Node {
        let space = self.lines[line].space;
        if space == parent.space {
            assert!(at + size <= parent.size, "{}: child {} at {at:#x}+{size:#x} past the parent's {:#x} bytes", parent.place, self.lines[line].name, parent.size);
            parent.kids.add(at, at + size);
        }
        let id = self.insts.len();
        self.insts.push(Inst { line: Some(line), space, size, kids: Vec::new() });
        self.insts[parent.id].kids.push(id);
        let group = self.lines[line].group.clone();
        Node {
            id,
            line: Some(line),
            size,
            space,
            holder: parent.holder.clone(),
            group,
            place: format!("{} +{at:#x}", parent.place),
            read: Ranges::default(), skipped: Ranges::default(),
            kids: Ranges::default(),
            error: None,
        }
    }

    /// One instance of `line` made of scattered `ranges` of `parent` (in the parent's space), such as every pack header of a
    /// program stream. Its bytes are addressed 0..total, in the order of `ranges`.
    pub fn pieces(&mut self, parent: &mut Node, line: LineId, ranges: &[(u64, u64)]) -> Node {
        let space = self.lines[line].space;
        let mut size = 0;
        for &(a, n) in ranges {
            if space == parent.space {
                assert!(a + n <= parent.size, "{}: piece of {} at {a:#x}+{n:#x} past the parent's {:#x} bytes", parent.place, self.lines[line].name, parent.size);
                parent.kids.add(a, a + n);
            }
            size += n;
        }
        let id = self.insts.len();
        self.insts.push(Inst { line: Some(line), space, size, kids: Vec::new() });
        self.insts[parent.id].kids.push(id);
        let group = self.lines[line].group.clone();
        Node {
            id,
            line: Some(line),
            size,
            space,
            holder: parent.holder.clone(),
            group,
            place: format!("{} ({} pieces)", parent.place, ranges.len()),
            read: Ranges::default(), skipped: Ranges::default(),
            kids: Ranges::default(),
            error: None,
        }
    }

    /// A region of `parent` with no line of its own (a block whose structures are its children): its holes are `group`'s unknown
    /// bytes.
    pub fn region(&mut self, parent: &mut Node, group: &str, at: u64, size: u64) -> Node {
        assert!(at + size <= parent.size, "{}: region {group} at {at:#x}+{size:#x} past the parent's {:#x} bytes", parent.place, parent.size);
        parent.kids.add(at, at + size);
        let id = self.insts.len();
        self.insts.push(Inst { line: None, space: parent.space, size, kids: Vec::new() });
        self.insts[parent.id].kids.push(id);
        Node {
            id,
            line: None,
            size,
            space: parent.space,
            holder: parent.holder.clone(),
            group: group.to_string(),
            place: format!("{} +{at:#x}", parent.place),
            read: Ranges::default(), skipped: Ranges::default(),
            kids: Ranges::default(),
            error: None,
        }
    }

    /// A root in unpacked space, held by `parent` (the LZ stream it was unpacked from): its holes are `group`'s unknown bytes.
    pub fn unpacked_root(&mut self, parent: &mut Node, group: &str, size: u64) -> Node {
        let id = self.insts.len();
        self.insts.push(Inst { line: None, space: Space::Unpacked, size, kids: Vec::new() });
        self.insts[parent.id].kids.push(id);
        Node {
            id,
            line: None,
            size,
            space: Space::Unpacked,
            holder: parent.holder.clone(),
            group: group.to_string(),
            place: format!("{} unpacked", parent.place),
            read: Ranges::default(), skipped: Ranges::default(),
            kids: Ranges::default(),
            error: None,
        }
    }

    /// Closes a node: its bytes go to its line (read or unread); a root's holes go to its group's unknown bytes.
    pub fn close(&mut self, n: Node) {
        let found = n.read.union(&n.kids);
        let covered = found.union(&n.skipped);
        match n.line {
            Some(id) => {
                let unread = n.size - covered.total().min(n.size);
                let skipped = covered.total().min(n.size) - found.total().min(n.size);
                let l = &mut self.lines[id];
                l.instances += 1;
                l.bytes += n.size;
                l.unread += unread;
                l.skipped += skipped;
                l.holders.insert(n.holder.clone());
                if let Some(e) = n.error {
                    l.n_errors += 1;
                    if l.errors.len() < 3 {
                        l.errors.push(format!("{}: {e}", n.place));
                    }
                }
            }
            None => {
                if let Some(e) = &n.error {
                    panic!("{}: a root node cannot fail: {e}", n.place);
                }
                let holes = covered.holes(n.size);
                if !holes.is_empty() {
                    let uid = self.unknown_line(&n.group, n.space);
                    for (a, b) in holes {
                        let id = self.insts.len();
                        self.insts.push(Inst { line: Some(uid), space: n.space, size: b - a, kids: Vec::new() });
                        self.insts[n.id].kids.push(id);
                        let l = &mut self.lines[uid];
                        l.instances += 1;
                        l.bytes += b - a;
                        l.holders.insert(n.holder.clone());
                    }
                }
            }
        }
    }

    fn unknown_line(&mut self, group: &str, space: Space) -> LineId {
        if let Some(&id) = self.unknown_index.get(group) {
            return id;
        }
        let id = self.line_of(group, "unknown bytes", "", "", space, Kind::Unknown);
        self.unknown_index.insert(group.to_string(), id);
        id
    }

    /// Checks the sector claims (no overlap; overlaps are a walk error) and lists the sectors no claim covers.
    pub fn finish_sectors(&mut self, total: u64) -> Result<(), String> {
        self.claims.sort();
        let mut at = 0;
        let mut gaps = Vec::new();
        let mut prev = String::new();
        for (a, b, what) in &self.claims {
            if *a < at {
                return Err(format!("sectors {a}..{b} ({what}) overlap {prev} ending at {at}"));
            }
            if *a > at {
                gaps.push((at, *a));
            }
            at = *b;
            prev = what.clone();
        }
        if at > total {
            return Err(format!("{prev} ends at sector {at}, past the disc's {total}"));
        }
        if at < total {
            gaps.push((at, total));
        }
        let id = self.line_of("Disc", "unassigned sectors", "", "", Space::Disc, Kind::Unassigned);
        for (a, b) in gaps {
            let l = &mut self.lines[id];
            l.instances += b - a;
            l.bytes += (b - a) * 0x800;
            l.holders.insert(format!("{a}..{b}"));
        }
        Ok(())
    }

    pub fn mapped(&self, id: LineId) -> bool {
        let l = &self.lines[id];
        l.kind == Kind::Data && l.instances > 0 && !l.decoder.is_empty() && !l.game.is_empty() && l.unread == 0 && l.n_errors == 0
    }

    /// A mapped line whose data the port has used.
    pub fn loaded(&self, id: LineId) -> bool {
        let l = &self.lines[id];
        self.mapped(id) && LOADED.iter().any(|&(g, n)| l.group == g && l.name.starts_with(n))
    }

    /// A line whose marks rest on partial evidence.
    pub fn claimed(&self, id: LineId) -> bool {
        let l = &self.lines[id];
        CLAIMED.iter().any(|&(d, n)| l.decoder.starts_with(d) && l.name.contains(n))
    }

    /// An instance and everything unpacked from it mapped.
    fn fully(&self, i: InstId, dec: &[bool]) -> bool {
        let x = &self.insts[i];
        x.line.is_none_or(|l| dec[l] || self.lines[l].kind == Kind::Ignored) && x.kids.iter().all(|&k| self.fully(k, dec))
    }

    pub fn totals(&self, overlays: usize, overlays_recompiled: usize) -> Totals {
        let dec: Vec<bool> = (0..self.lines.len()).map(|i| self.mapped(i)).collect();
        let (mut proven_bytes, mut claimed_bytes) = (0, 0);
        for x in self.insts.iter() {
            let Some(l) = x.line else { continue };
            if x.space != Space::Disc || self.lines[l].kind != Kind::Data || !dec[l] {
                continue;
            }
            // Own bytes: the instance minus its children on the disc; children in unpacked space must be decoded too.
            let disc_kids: u64 = x.kids.iter().filter(|&&k| self.insts[k].space == Space::Disc).map(|&k| self.insts[k].size).sum();
            let unpacked_ok = x.kids.iter().filter(|&&k| self.insts[k].space == Space::Unpacked).all(|&k| self.fully(k, &dec));
            if unpacked_ok {
                if self.claimed(l) || self.unpacked_claimed(x) {
                    claimed_bytes += x.size - disc_kids;
                } else {
                    proven_bytes += x.size - disc_kids;
                }
            }
        }
        let shown: Vec<&Line> = self.lines.iter().filter(|l| l.instances > 0).collect();
        Totals {
            data_lines: shown.iter().filter(|l| !matches!(l.kind, Kind::Code { .. } | Kind::Excluded | Kind::Ignored)).count(),
            mapped_lines: (0..self.lines.len()).filter(|&i| dec[i]).count(),
            loaded_lines: (0..self.lines.len()).filter(|&i| self.loaded(i)).count(),
            proven_bytes,
            claimed_bytes,
            unknown_bytes: shown.iter().filter(|l| l.kind == Kind::Unknown).map(|l| l.bytes).sum(),
            unassigned_sectors: shown.iter().filter(|l| l.kind == Kind::Unassigned).map(|l| l.instances).sum(),
            code_bytes: shown.iter().filter(|l| matches!(l.kind, Kind::Code { .. })).map(|l| l.bytes).sum(),
            excluded_bytes: shown.iter().filter(|l| l.kind == Kind::Excluded).map(|l| l.bytes).sum(),
            ignored_bytes: shown.iter().map(|l| if l.kind == Kind::Ignored { l.bytes } else { l.skipped }).sum(),
            overlays_recompiled,
            overlays,
        }
    }

    /// Whether anything unpacked from instance `x` sits on a claimed line (its disc bytes then count as claimed).
    fn unpacked_claimed(&self, x: &Inst) -> bool {
        x.kids.iter().any(|&k| {
            let c = &self.insts[k];
            c.space == Space::Unpacked && (c.line.is_some_and(|l| self.claimed(l)) || self.unpacked_claimed(c))
        })
    }

    /// docs/decode.md.
    pub fn report(&self, disc_bytes: u64, overlays: usize, overlays_recompiled: usize, header: &str) -> String {
        let t = self.totals(overlays, overlays_recompiled);
        let mut s = String::new();
        let _ = writeln!(s, "# Decode tracker\n");
        let _ = writeln!(
            s,
            "Data: {} of {} lines mapped, {} of them loaded by the port. Of {} disc bytes, {} mapped on proven evidence and {} claimed. {} unknown bytes. {} unassigned sectors.",
            t.mapped_lines,
            t.data_lines,
            t.loaded_lines,
            commas(disc_bytes),
            commas(t.proven_bytes),
            commas(t.claimed_bytes),
            commas(t.unknown_bytes),
            commas(t.unassigned_sectors)
        );
        let _ = writeln!(s, "Code: {} of {} level overlays recompiled.\n", t.overlays_recompiled, t.overlays);
        s.push_str(header);
        let _ = writeln!(
            s,
            "\nCode lines hold {} bytes and excluded lines {} bytes (the multiplayer levels, the online-play files and the Sly 2 demo, left out by the player); both are counted apart and are not in the data line's bytes. Bytes nothing in the game reads are ignored by the player's decision: {} bytes, on `ignored` lines and as skipped bytes of data lines (their notes give the count); they do not count as unread.\n",
            commas(t.code_bytes),
            commas(t.excluded_bytes),
            commas(t.ignored_bytes)
        );
        // Groups in first-seen order.
        let mut order: Vec<&str> = Vec::new();
        let mut by: BTreeMap<&str, Vec<LineId>> = BTreeMap::new();
        for (i, l) in self.lines.iter().enumerate() {
            if l.instances == 0 {
                continue;
            }
            if !order.contains(&l.group.as_str()) {
                order.push(&l.group);
            }
            by.entry(&l.group).or_default().push(i);
        }
        for g in order {
            let ids = &by[g];
            let _ = writeln!(s, "## {g}\n");
            let _ = writeln!(s, "| Structure | Held by | Bytes | State | Decoder | Game function | Notes |");
            let _ = writeln!(s, "| --- | --- | --- | --- | --- | --- | --- |");
            for &i in ids {
                let l = &self.lines[i];
                let bytes = format!("{}{}", commas(l.bytes), if l.space == Space::Unpacked { " unpacked" } else { "" });
                let state = match l.kind {
                    Kind::Code { recompiled: true } => "recompiled".to_string(),
                    Kind::Code { recompiled: false } => "not recompiled".to_string(),
                    Kind::Unknown => "unknown".to_string(),
                    Kind::Unassigned => "unassigned".to_string(),
                    Kind::Excluded => "excluded".to_string(),
                    Kind::Ignored => "ignored (read by nothing)".to_string(),
                    Kind::Data if self.loaded(i) && self.claimed(i) => "mapped, loaded, claimed".to_string(),
                    Kind::Data if self.loaded(i) => "mapped, loaded".to_string(),
                    Kind::Data if self.mapped(i) && self.claimed(i) => "mapped, claimed".to_string(),
                    Kind::Data if self.mapped(i) => "mapped".to_string(),
                    Kind::Data => {
                        let mut why = Vec::new();
                        if l.decoder.is_empty() {
                            why.push("no decoder".to_string());
                        } else if l.unread > 0 {
                            why.push(format!("{} bytes unread", commas(l.unread)));
                        }
                        if l.n_errors > 0 {
                            why.push(format!("{} errors", l.n_errors));
                        }
                        if l.game.is_empty() {
                            why.push("no game function".to_string());
                        }
                        format!("not mapped: {}", why.join(", "))
                    }
                };
                let mut notes = l.note.clone();
                if l.skipped > 0 && l.kind != Kind::Ignored {
                    if !notes.is_empty() {
                        notes.push_str("; ");
                    }
                    notes.push_str(&format!("{} bytes read by nothing (skipped)", commas(l.skipped)));
                }
                for e in &l.errors {
                    if !notes.is_empty() {
                        notes.push_str("; ");
                    }
                    notes.push_str(&format!("error {e}"));
                }
                let _ = writeln!(
                    s,
                    "| {} | {} | {} | {} | {} | {} | {} |",
                    l.name.replace('|', "/"),
                    holders(l),
                    bytes,
                    state,
                    if l.decoder.is_empty() { "-" } else { l.decoder },
                    if l.game.is_empty() { "-" } else { l.game },
                    notes.replace('|', "/")
                );
            }
            s.push('\n');
        }
        s
    }
}

impl Default for Tracker {
    fn default() -> Self {
        Self::new()
    }
}

/// "51 levels", "WAD3", "SCUS_973.53", or a short list.
fn holders(l: &Line) -> String {
    if l.kind == Kind::Unassigned {
        let v: Vec<&String> = l.holders.iter().collect();
        return format!("{} ranges", v.len());
    }
    let levels = l.holders.iter().filter(|h| h.len() == 3 && h.starts_with('L') && h[1..].chars().all(|c| c.is_ascii_digit())).count();
    if levels == l.holders.len() && levels > 1 {
        return format!("{levels} levels");
    }
    let v: Vec<&str> = l.holders.iter().map(|s| s.as_str()).collect();
    if v.len() <= 4 {
        return v.join(", ");
    }
    format!("{}, ... ({} files)", v[..3].join(", "), v.len())
}

pub fn commas(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}
