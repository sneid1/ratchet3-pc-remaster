"""Builds the button prompt atlas `uya play` draws in place of the game's PS2 button glyphs (port/uya/src/prompts.rs), from the
player's downloaded icons: Kenney's Input Prompts 1.5 (CC0, kenney.nl), the pack Mirrorshard's prompts come from. Nothing is
downloaded or drawn anew: each icon's own shape is recoloured to the game's glyph style (a dark disc or keycap with a coloured or
light symbol, as the game's own glyph sheet and L1 / R1 / L2 / R2 keycaps are) and kept at its own shape, ICON_H high; the game
fits it into the glyph it replaces keeping that shape (prompts.rs `fitted`).

The prompts show what the player bound (port/uya/src/bindings.rs): so the atlas has an icon for every key and mouse button a
binding takes (named as bindings.rs names them: winit's key names, "KeyW", "ShiftLeft", and "MouseLeft" ... "MouseForward"), the
mouse's motion ("MouseMove", the right stick) and a blank cell the game draws the four move keys bound into ("Moves", the left
stick); and every button of an Xbox controller, a DualSense and a DualShock 4, each named by the PS2 button it presses unbound
("cross", "l1", "select", "up"), the sticks too ("left_stick", "right_stick"). The pads show the icon of the button bound. uya checks at its start that every input it can bind has its icon here.

Usage:  python -I scripts/prompt-icons.py <Kenney "x" folder> <out atlas file>
        e.g. python -I scripts/prompt-icons.py "C:/Users/<you>/osm2streets-build/.local/downloads/kenney-input-prompts/x"
             "C:/Users/<you>/Games/Ratchet and Clank 3/prompts.bin"

Atlas file (little-endian u32 words): "UYAP", version 2, width, height, cell count; per cell its device (1 Xbox, 2 keyboard and
mouse, 3 DualSense, 4 DualShock 4), its name (16 bytes, NUL-padded ASCII), x, y, width, height; then width x height RGBA pixels,
rows top first (the colours as shown, alpha 255 opaque). Cells of the same picture share its pixels.
"""
import struct
import sys
from pathlib import Path

from PIL import Image

KENNEY = Path(sys.argv[1])
OUT = Path(sys.argv[2])
XBOX = KENNEY / "Xbox Series" / "Double"
KEYS = KENNEY / "Keyboard & Mouse" / "Double"
PS = KENNEY / "PlayStation Series" / "Double"

# The game's glyph style: the disc / keycap body and the light symbol (its keycap palette's greys, as shown).
BODY = (30, 30, 32)
LIGHT = (205, 205, 205)
# Xbox face button letters; the PlayStation symbols.
GREEN, RED, BLUE, YELLOW = (107, 190, 69), (232, 72, 60), (64, 140, 232), (240, 196, 25)
PS_BLUE, PS_RED, PS_PINK, PS_GREEN = (124, 178, 232), (232, 72, 60), (222, 128, 200), (64, 200, 160)

# Every icon this high (pixels), with a transparent edge of EDGE around it in its cell (the game clamps its samples to the cell).
ICON_H = 56
EDGE = 2
# The blank cell for the move keys (the game draws them in: prompts.rs `Atlas::with_moves`): three keys across, two down.
MOVES = (126, 84)


def recolour(path: Path, symbol) -> Image.Image:
    """The icon's shape with its body BODY and its cut-out symbol filled with `symbol`; coloured parts (a mouse button's red) kept."""
    im = Image.open(path).convert("RGBA")
    w, h = im.size
    px = im.load()
    # Outside: transparent or edge pixels reachable from the border; the holes the body encloses are the symbol.
    outside = [[False] * w for _ in range(h)]
    stack = [(x, y) for x in range(w) for y in (0, h - 1)] + [(x, y) for y in range(h) for x in (0, w - 1)]
    while stack:
        x, y = stack.pop()
        if outside[y][x] or px[x, y][3] >= 250:
            continue
        outside[y][x] = True
        for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            a, b = x + dx, y + dy
            if 0 <= a < w and 0 <= b < h and not outside[b][a]:
                stack.append((a, b))
    out = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    o = out.load()
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            white = min(r, g, b) > 180 and max(r, g, b) - min(r, g, b) < 30
            if outside[y][x]:
                if a:
                    o[x, y] = (*(BODY if white else (r, g, b)), a)
                continue
            # Inside the silhouette: the body where the icon is opaque, the symbol where it is see-through, blended between.
            k = a / 255
            base = BODY if white or a < 250 else (r, g, b)
            o[x, y] = tuple(round(base[c] * k + symbol[c] * (1 - k)) for c in range(3)) + (255,)
    return out.crop(out.getbbox())


def sized(icon: Image.Image) -> Image.Image:
    """The icon ICON_H high at its own shape, with a transparent EDGE around it."""
    w = max(1, round(icon.width * ICON_H / icon.height))
    icon = icon.resize((w, ICON_H), Image.LANCZOS)
    im = Image.new("RGBA", (w + 2 * EDGE, ICON_H + 2 * EDGE), (0, 0, 0, 0))
    im.alpha_composite(icon, (EDGE, EDGE))
    return im


def key(file: str) -> Image.Image:
    return recolour(KEYS / f"{file}.png", LIGHT)


# The keyboard's keys (bindings.rs KEYS, by winit's names) and their icons.
keyboard = {f"Key{c.upper()}": f"keyboard_{c}" for c in "abcdefghijklmnopqrstuvwxyz"}
keyboard.update({f"Digit{d}": f"keyboard_{d}" for d in range(10)})
keyboard.update({f"Numpad{d}": f"keyboard_{d}" for d in range(10)})
keyboard.update({f"F{n}": f"keyboard_f{n}" for n in range(1, 13)})
keyboard.update({
    "ArrowUp": "keyboard_arrow_up", "ArrowDown": "keyboard_arrow_down", "ArrowLeft": "keyboard_arrow_left", "ArrowRight": "keyboard_arrow_right",
    "Space": "keyboard_space_icon", "Enter": "keyboard_enter", "Tab": "keyboard_tab_icon", "Backspace": "keyboard_backspace_icon",
    "Escape": "keyboard_escape", "CapsLock": "keyboard_capslock_icon",
    "ShiftLeft": "keyboard_shift_icon", "ShiftRight": "keyboard_shift_icon", "ControlLeft": "keyboard_ctrl", "ControlRight": "keyboard_ctrl",
    "AltLeft": "keyboard_alt", "AltRight": "keyboard_alt",
    "Backquote": "keyboard_tilde", "Minus": "keyboard_minus", "Equal": "keyboard_equals", "BracketLeft": "keyboard_bracket_open",
    "BracketRight": "keyboard_bracket_close", "Backslash": "keyboard_slash_back", "Semicolon": "keyboard_semicolon",
    "Quote": "keyboard_apostrophe", "Comma": "keyboard_comma", "Period": "keyboard_period", "Slash": "keyboard_slash_forward",
    "Insert": "keyboard_insert", "Delete": "keyboard_delete", "Home": "keyboard_home", "End": "keyboard_end",
    "PageUp": "keyboard_page_up", "PageDown": "keyboard_page_down",
    "NumpadAdd": "keyboard_numpad_plus", "NumpadSubtract": "keyboard_minus", "NumpadMultiply": "keyboard_asterisk",
    "NumpadDivide": "keyboard_slash_forward", "NumpadDecimal": "keyboard_period", "NumpadEnter": "keyboard_numpad_enter",
    "MouseLeft": "mouse_left", "MouseRight": "mouse_right", "MouseMiddle": "mouse_scroll", "MouseBack": "mouse_side_back",
    "MouseForward": "mouse_side_forward", "MouseMove": "mouse_move",
})

# A controller's buttons by the PS2 button each presses unbound (bindings.rs PAD), and the sticks.
xbox = {
    "cross": recolour(XBOX / "xbox_button_a.png", GREEN), "circle": recolour(XBOX / "xbox_button_b.png", RED),
    "square": recolour(XBOX / "xbox_button_x.png", BLUE), "triangle": recolour(XBOX / "xbox_button_y.png", YELLOW),
    "l1": "xbox_lb", "r1": "xbox_rb", "l2": "xbox_lt", "r2": "xbox_rt", "l3": "xbox_stick_l_press", "r3": "xbox_stick_r_press",
    "select": "xbox_button_view", "start": "xbox_button_menu",
    "up": "xbox_dpad_up", "down": "xbox_dpad_down", "left": "xbox_dpad_left", "right": "xbox_dpad_right",
    "left_stick": "xbox_stick_l", "right_stick": "xbox_stick_r",
}
# The PlayStation face buttons as the game's own glyphs are: a dark disc, the symbol in its colour.
playstation = {
    "cross": recolour(PS / "playstation_button_cross.png", PS_BLUE), "circle": recolour(PS / "playstation_button_circle.png", PS_RED),
    "square": recolour(PS / "playstation_button_square.png", PS_PINK), "triangle": recolour(PS / "playstation_button_triangle.png", PS_GREEN),
    "l1": "playstation_trigger_l1", "r1": "playstation_trigger_r1", "l2": "playstation_trigger_l2", "r2": "playstation_trigger_r2",
    "l3": "playstation_button_l3", "r3": "playstation_button_r3",
    "up": "playstation_dpad_up", "down": "playstation_dpad_down", "left": "playstation_dpad_left", "right": "playstation_dpad_right",
    "left_stick": "playstation_stick_l", "right_stick": "playstation_stick_r",
}
dualsense = dict(playstation, select="playstation5_button_create", start="playstation5_button_options")
dualshock4 = dict(playstation, select="playstation4_button_share", start="playstation4_button_options")

# (device, name, picture key): each picture made once.
pictures = {}
cells = []


def add(device, name, src, folder):
    k = (folder, src) if isinstance(src, str) else ("picture", id(src))
    if k not in pictures:
        pictures[k] = sized(recolour(folder / f"{src}.png", LIGHT) if isinstance(src, str) else src)
    cells.append((device, name, k))


for name, file in keyboard.items():
    add(2, name, file, KEYS)
pictures["moves"] = Image.new("RGBA", MOVES, (0, 0, 0, 0))
cells.append((2, "Moves", "moves"))
for name, src in xbox.items():
    add(1, name, src, XBOX)
for device, table in ((3, dualsense), (4, dualshock4)):
    for name, file in table.items():
        add(device, name, file, PS)

# Packed in rows, tallest first; a 1-pixel transparent gap between pictures.
W = 1024
order = sorted(pictures, key=lambda k: (-pictures[k].height, -pictures[k].width))
place = {}
x = y = row_h = 0
for k in order:
    im = pictures[k]
    if x + im.width + 1 > W:
        x, y = 0, y + row_h
        row_h = 0
    place[k] = (x, y)
    x += im.width + 1
    row_h = max(row_h, im.height + 1)
H = 1
while H < y + row_h:
    H *= 2
assert H <= 1024, f"the atlas needs {y + row_h} rows, more than 1024"
atlas = Image.new("RGBA", (W, H), (0, 0, 0, 0))
for k, (px, py) in place.items():
    atlas.alpha_composite(pictures[k], (px, py))
OUT.parent.mkdir(parents=True, exist_ok=True)
with open(OUT, "wb") as f:
    f.write(b"UYAP" + struct.pack("<4I", 2, W, H, len(cells)))
    for device, name, k in cells:
        assert len(name) <= 16, name
        px, py = place[k]
        f.write(struct.pack("<I", device) + name.encode("ascii").ljust(16, b"\0") + struct.pack("<4I", px, py, pictures[k].width, pictures[k].height))
    f.write(atlas.tobytes())
atlas.save(OUT.with_suffix(".png"))
print(f"{OUT}: {W}x{H}, {len(cells)} icons from {len(pictures)} pictures; preview {OUT.with_suffix('.png')}")
