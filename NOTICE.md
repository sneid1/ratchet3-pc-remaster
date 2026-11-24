# Notices and credits

This project is licensed under GPL-3.0-or-later (see [LICENSE](LICENSE)), with one additional permission for the NVIDIA DLSS files (see "Additional permission under GNU GPL version 3 section 7" below). It builds on the published knowledge and work listed below.

## The game's translated code

The level libraries the game loads (`tr_l01.dll` … `tr_l36.dll`) are the disc's own machine code, translated ahead of time by this project's translator and shipped built in the releases. Ratchet & Clank: Up Your Arsenal is © Sony Interactive Entertainment and Insomniac Games. The releases are of no use without the game: you need your own copy of the USA disc (SCUS-97353). The game's assets, the disc image and anything extracted from them are not distributed here or in the releases.

## The releases' updated-textures volumes

The releases ship the remaster's updated textures as split volumes (`updated-textures.7z.001` …). These are updated versions of the game's own textures: the remaster's redraw and material work on the original art, and they are of no use without the game. Inside them, `texture_replacements/` also carries two community packs, whose updated versions of the game's textures remain their authors' work:

- reddyxd and nerus's Up Your Arsenal texture pack (gbatemp.net, thread 631588);
- texmaster's PAL gapfill.

## NVIDIA's DLSS and Streamline files

The DLL files that come with the release zip sit next to `uya.exe`; they are not in the source repository. The game works without them (DLSS and frame generation then stay off).

- `nvngx_dlss.dll` and `nvngx_dlssg.dll` are the NVIDIA DLSS runtimes, redistributed unmodified from the DLSS SDK. They are not covered by the GPL: they are licensed to you under the NVIDIA RTX SDKs licence, with its terms.
- `sl.interposer.dll`, `sl.common.dll`, `sl.dlss_g.dll`, `sl.reflex.dll` and `sl.pcl.dll` are the unmodified production binaries of NVIDIA Streamline SDK 2.14.16 (https://github.com/NVIDIA-RTX/Streamline), under the MIT licence.
- `NvLowLatencyVk.dll` ships beside them from the same SDKs.

NVIDIA, the NVIDIA logo, GeForce, RTX and DLSS are trademarks and/or registered trademarks of NVIDIA Corporation in the U.S. and other countries. This project is not affiliated with, endorsed by or sponsored by NVIDIA.

### Additional permission under GNU GPL version 3 section 7

If you modify this project, or any covered work, by linking or combining it with the NVIDIA DLSS runtime (`nvngx_dlss.dll`) or the NVIDIA DLSS Frame Generation runtime (`nvngx_dlssg.dll`) (or a modified version of those libraries), containing parts covered by the terms of the NVIDIA RTX SDKs licence, the licensors of this project grant you additional permission to convey the resulting work. This permission covers only those two libraries: they are not part of the Corresponding Source of this project, and everything else stays under GPL-3.0-or-later.

## rac-rs

`patches/rac-rs/` patch rac-rs v0.5.1 (AsobiCodes, GPL-3.0-or-later), and `port/uyaref` (the emulated reference, never what players run) is built on it.

## OpenRAC's rac3-decomp

The source of the game code's function bounds and names.

## PCSX2

The reference emulator the port is set against.
