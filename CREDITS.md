# Credits

mcsm-rs is licensed GPL-3.0-or-later (see `LICENSE`). It contains no game data: everything is read at
runtime from the user's own installed copy of *Minecraft: Story Mode* (Telltale Games / Mojang).
*Minecraft: Story Mode* and Telltale Tool are trademarks of their respective owners; this project is
not affiliated with or endorsed by them.

## Code ported or adapted

| Source | Author | License | What we used | Where |
|---|---|---|---|---|
| [ttarchext](https://aluigi.altervista.org/papers.htm#ttarchext) ([mirror](https://github.com/Keyaku/ttarchext)) | Luigi Auriemma | GPL-2.0-or-later | Telltale "version 7" Blowfish round order; ECTT/ZCTT/NCTT chunk container and 4ATT/3ATT index layout | `crates/ttarch/src/blowfish.rs`, `crates/ttarch/src/archive.rs` |
| [TTG Tools](https://github.com/HeitorSpectre/TTG-Tools) | HeitorSpectre and contributors | GPL-3.0 | Script decryption (`LEn`/`LEo`), T3Texture header field order and surface format codes, Font and LanguageDB record layouts | `crates/ttarch/src/script.rs`, `crates/ttmeta/src/texture.rs`, `crates/ttmeta/src/font.rs`, `crates/ttmeta/src/text.rs` |
| [Telltale D3DMesh Editor](https://github.com/HeitorSpectre/Telltale-D3DMesh-Editor) | Heitor Spectre | GPL-3.0 | D3DMesh version-18 field order (`ParseV18`), vertex attribute and UV formats | `crates/ttmeta/src/mesh.rs` |
| Blowfish reference C ([di-mgt.com.au](https://www.di-mgt.com.au/crypto.html)), via ttarchext | Bruce Schneier / David Ireland | "may be freely distributed" | Standard Blowfish structure underlying the above | `crates/ttarch/src/blowfish.rs` |

## Libraries

| Crate | License | Used for |
|---|---|---|
| [fsbex](https://github.com/astral4/fsbex) | MIT OR Apache-2.0 | Reading FMOD FSB5 streams from the user's `.bank` files as Ogg Vorbis |
| [mlua](https://github.com/mlua-rs/mlua), bundled [Lua 5.2](https://www.lua.org/license.html) | MIT | Executing installed inline dialogue scripts with a limited engine API and instruction budget |
| [rfd](https://github.com/PolyMeilex/rfd) | MIT | The "choose your game folder" dialog on first start |
| [Bevy](https://bevyengine.org), [flate2](https://github.com/rust-lang/flate2-rs), [half](https://github.com/starkat99/half-rs), [thiserror](https://github.com/dtolnay/thiserror) | MIT OR Apache-2.0 | Engine, deflate, half floats, errors |

## Documentation and research consulted

| Source | Author | License | Used for |
|---|---|---|---|
| [ttarch-docs](https://github.com/Telltale-Modding-Group/ttarch-docs) | Telltale Modding Group | MIT | TTCN/4ATT container reading guide |
| [ttmod](https://github.com/blazebsc/ttmod) | blazebsc | MIT | MCSM boot/Lua pipeline notes (Lua 5.2.3, `\x1bLEo` scripts, resdesc loading) |
| [TelltaleLuaScriptingDocumentation](https://github.com/frostbone25/TelltaleLuaScriptingDocumentation) | frostbone25 | MIT | Engine Lua API surface (planned) |
| [TelltaleInspector](https://github.com/LucasSaragosa/TelltaleInspector) | Lucas Saragosa | MIT | Telltale Tool library formats (planned) |
| [TelltaleToolKit](https://github.com/iMrShadow/TelltaleToolKit) | iMrShadow | MIT | Skeleton entry member order (`crates/ttmeta/src/skeleton.rs`, our own code). The compressed skeleton key layout was read from the game's own decoder instead |
| [Telltale Editor](https://github.com/LucasSaragosa/Telltale-Editor), [TelltaleToolLib](https://github.com/LucasSaragosa/TelltaleToolLib) | Lucas Saragosa | no license file: **reference only, nothing copied** | Format understanding: Animation, Chore, dialogue node / child, visibility-rule and logic-group member order (`crates/ttmeta/src/anim.rs`, `crates/ttmeta/src/chore.rs`, `crates/ttmeta/src/dialog.rs` are our own code; block-wrapping and byte layout verified against the user's files) |

## Tools used during research (not part of this project)

| Tool | License | Used for |
|---|---|---|
| [unluac](https://sourceforge.net/projects/unluac/) | MIT | Reading the game's decrypted Lua 5.2 bytecode to learn menu behaviour; output stays outside the repo |
| [Ghidra](https://github.com/NationalSecurityAgency/ghidra) via [ghidra-cli](https://github.com/akiselev/ghidra-cli) | Apache-2.0 / GPL-3.0 | Static analysis of the user's executable |

## Our own analysis

Static analysis of the user's `MinecraftStoryMode.exe` (Ghidra 12.1.4 via ghidra-cli) located the Blowfish
key schedule (`0x005302a0`), the key and tables it uses, and the ECTT/ZCTT/NCTT reader (`0x0059bc40`).
MetaStream container, PropertySet encoding (including the block-wrapping rules), Scene layout,
the CRC-64 symbol scheme, resource-set priorities, the right-handed / camera-looks-down-+Z
convention and the menu layout behaviour were worked out from the user's own files.
The opening story research also identified dialogue language-resource cues and single-stream FSB5
MPEG voice containers (four-byte-aligned MP3 frames). The voice depadding code is our own.
Findings are written up as behaviour, never as decompiled code. The key and tables are read from the
user's exe at runtime, not stored here.
