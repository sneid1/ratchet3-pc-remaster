# Ratchet & Clank: Up Your Arsenal — PC remaster

A native PC port of Ratchet & Clank: Up Your Arsenal (PS2, 2004), rebuilt from the game's own code: the disc's machine code is translated ahead of time and runs on this project's own runtime and renderer, with native services where the PS2 had hardware. No emulation anywhere. You bring your own copy of the game — the USA disc, serial SCUS-97353 — and the disc stays in play the whole time, streaming what it streamed on the PlayStation 2.

## What it looks like

<!-- screenshots once the fresh captures land -->

## The remaster

- **Updated textures** (on by default): the world and its characters redrawn — 4,990 surfaces across 1,134 new materials, each with surface grain, normal-mapped relief and gloss, lit per pixel at the material's own look; 6,623 textures carry generated height, so brick, bark and sand catch the light. Turn it off and the game draws its own picture, unchanged.
- **Ray tracing** (needs DLSS and an RTX GPU):
  - **Ray-traced shadows**: the level's sun through the terrain, every tie, shrub and character, each leaf and blade cut out by its own texture — sharp at every distance, in between-frame pictures too.
  - **Ray-traced lighting**: light bounced off sunlit walls, water and glowing surfaces into the shade, with the ambient occlusion traced as well.
  - **Ray-traced reflections**: water, swamp, slime and the game's chrome reflect the whole level — including what is off screen.
- **DLSS** (NVIDIA RTX): Super Resolution (DLAA, Quality, Balanced, Performance) draws the world smaller and rebuilds it sharp; **Frame Generation** (2X–4X, through Streamline) makes frames between the drawn ones, up to the screen's rate. The HUD and menus are drawn after DLSS at full size, so they stay sharp.
- **The rest of the graphics page**: per-pixel lighting, ambient occlusion, water reflections, sun light shafts, a far draw distance that stops pop-in, texture filtering to 16x, widescreen at the screen's own shape, and a field-of-view option whose culls and targeting follow it.
- **A PC game throughout**: the game's own front end and pause menus with a PC settings page inside them, keyboard and mouse (menus by pointer and click, aiming with the mouse), Xbox / DualSense / DualShock 4 with their own button prompts, rebindable keys, saves as plain files, and 59.94 fields of game time a second with a picture at every refresh above 60 Hz.

## Requirements

- Windows 10 or 11, 64-bit.
- A Direct3D 12 / Vulkan GPU. DLSS and the ray-traced options need an NVIDIA RTX card (frame generation an RTX 40 or 50).
- Your own disc image of the USA release (SCUS-97353).

## Install

**With [RecompLauncher](https://github.com/elripalda/RecompLauncher)** (recommended): install the game, drop your `.iso` on the setup screen when asked, press Play. The release's `updated-textures` volumes are optional: download them from the release page and extract them into the game's folder (a README inside each volume says where).

**By hand**: download `ratchet3-pc-remaster-<version>-Windows-x64.zip` from [Releases](https://github.com/sneid1/ratchet3-pc-remaster/releases), extract it anywhere, and run `uya.exe`:

```
uya.exe play C:\path\to\SCUS_973.53.iso
```

Without the textures volumes the game plays with its own textures; with them, it finds them beside the program. Saves, settings and logs are written next to the program by default; `--profile DIR` moves them.

## Building from source

- A Rust toolchain (`x86_64-pc-windows-msvc`) and Visual Studio's Build Tools.
- Four folders named in the environment of the build, for NVIDIA's SDKs (`docs/WORKFLOW.md`, "DLSS"): `DLSS_SDK` (NVIDIA's DLSS SDK), `VULKAN_SDK` (a folder whose `Include` holds `vulkan\`), `LIBCLANG_PATH` (a folder with `libclang.dll`) and `STREAMLINE_SDK` (NVIDIA's Streamline SDK).
- `powershell -ExecutionPolicy Bypass -File scripts\build.ps1` builds the game (`port\`): `uya.exe` with its own crates `uyafmt`, `uyart`, `uycpu`. `scripts\package-release.ps1` assembles a release folder, zip and textures volumes.
- The level libraries (`tr_l01.dll` … `tr_l36.dll`) are the game's own code, translated from the disc by this project's translator; they are built from your own copy of the game (`scripts\uyatr.ps1`, then `scripts\build-levels.ps1`) and are not in the repository. The release ships them built.

## Repository layout

| | |
| --- | --- |
| `port/uya` | the game: its runtime, renderer, sound engine, settings and window (`uya play`, `uya native`) |
| `port/uyart`, `port/uycpu` | the runtime the translated code runs on: kernel, hardware, IOP servers, VU0 as native code |
| `port/uyafmt` | the game's own format decoders, written for this game's data |
| `port/uyatr` | the translator: the disc's EE code to Rust, ahead of time |
| `port/uyaref` | the emulated reference (built on rac-rs), the instrument the port is checked against — never what players run |
| `scripts/` | the build, the recompiler and translator drivers, the texture and model pipelines |
| `patches/rac-rs/` | the patches behind `uyaref`'s base |
| `textures/` | the texture packs' index files |
| `docs/` | how the port is built on, its workflow, and its design notes |

`disc/`, `generated/`, `translated/` and the build outputs stay local (`.gitignore`); nothing from the game is in this repository.

## License

This project's code is GPL-3.0 ([LICENSE](LICENSE)). The releases contain the game's code recompiled from the original disc and are of no use without the game: you need your own copy of Ratchet & Clank: Up Your Arsenal. Not affiliated with or endorsed by Insomniac Games or Sony Interactive Entertainment. Ratchet & Clank is their property.

Built on ideas shared with [rac-rs](https://github.com/AsobiCodes/rac-rs) (whose recompiler and PS2 core drove the early reference) and [OpenRAC's rac3-decomp](https://github.com/OpenRAC/rac3-decomp) (function bounds and names); PCSX2 as the reference emulator. The release's texture folders include two community texture packs ([reddyxd's UYA pack](https://www.pcsx2.net/) and texmaster's PAL gapfill) alongside this project's own updated textures.
