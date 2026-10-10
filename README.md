# mcsm-rs

A Rust + [Bevy](https://bevyengine.org) rewrite of the Telltale Tool engine as used by
*Minecraft: Story Mode* (PC), running the game from **your own installed copy**.

This repository contains no game data, no keys and no decompiled code. At startup it reads your
`MinecraftStoryMode.exe` (for the archive key and cipher tables) and your `Archives/*.ttarch2`.

## Status

| Milestone | State |
|---|---|
| Read every `.ttarch2` archive (ECTT/ZCTT/NCTT, 3ATT/4ATT) | done: 182/182 archives, 194,671 files on the 2016.5.17.1283 PC build |
| Bevy `ttarch://` asset source | done |
| Script decryption (`\x1bLEn`/`\x1bLEo` -> Lua 5.2 bytecode) | done: 1,020/1,020 scripts |
| Resource priorities from `_resdesc_50_*.lua` | done: patches override base, language packs excluded |
| Scenes, props (with inheritance), textures, static meshes, fonts | done: every file of each kind parses |
| Main menu (key art, title, list buttons, keyboard/mouse, Exit) | done (Milestone A) |
| Animations (`.anm`, keyframed) and chores (`.chore`) | done: 25,008/25,008 animations, 17,832/17,832 chores parse |
| Animated menu background (key-art chore: intro montage, then the resting shot) | done |
| Menu music (FMOD bank -> Ogg Vorbis, looped) | done |
| Skeletons (`.skl`), skinned meshes, skeletal animation (compressed and uncompressed keys) | done: 314/314 skeletons; 14,193 skeletal animations decode |
| Menu: key-art characters (townsfolk idles, chickens, chore-driven actions) | done |
| Character select (first Play): six looks, live texture / gender swap, look carried to the episode diorama | done (choice kept for the session only) |
| Episode select (Play): diorama per episode, sliding between episodes, title / status / arrows | done (starting an episode is not implemented) |
| Settings menus (Audio/Video, Gameplay, Graphics; toggles, sliders, Apply/Restore popup), saved to `%APPDATA%\mcsm-rs\settings.txt` | done (volumes, full screen, resolution, anti-aliasing and brightness take effect) |
| Menu: held props (agents attached to bones), character lighting, news feed, FMOD event logic | next |
| Lua runtime + engine API, dialogue | planned |

## Running

Point it at your install with any one of:

```text
cargo run -p mcsm --release -- "D:\path\to\Minecraft Story Mode"
set MCSM_GAME_DIR=D:\path\to\Minecraft Story Mode
copy mcsm.example.toml mcsm.toml   (then edit game_dir)
```

The path may be the game folder or `MinecraftStoryMode.exe` itself.

Developer CLI:

```text
cargo run -p mcsm-tools --release -- info
cargo run -p mcsm-tools --release -- ls Boot_data
cargo run -p mcsm-tools --release -- find <file-name>
cargo run -p mcsm-tools --release -- extract <file-name> <folder-outside-the-repo>
cargo run -p mcsm-tools --release -- meta <file.scene|.prop|.d3dmesh|.d3dtx|.font>
```

Tests that touch real data run only when `MCSM_GAME_DIR` is set, and assert structure, never content.

## Layout

| Path | What |
|---|---|
| `crates/ttarch` | archive reader, Telltale Blowfish, key lookup in the exe, scripts, resource sets, install/VFS |
| `crates/ttmeta` | MetaStream readers: properties, scenes, textures, meshes, fonts, text |
| `crates/mcsm` | the Bevy game |
| `crates/mcsm-tools` | developer CLI |
| `sheets/` | project sheets (doctrine, plan, decisions, kernel allowlist) per the Spreadsheet Method |

## Rules for contributors

- `.gitignore` is a **whitelist**. Anything not explicitly allowed is ignored, so extracted assets
  cannot be committed by accident. Add new file kinds to it deliberately.
- Never paste game assets, decrypted scripts, keys, or decompiler output into the repo.
  Describe behaviour in your own words and code.
- Credit every source you learn from in `CREDITS.md`, with its license. Code from sources without
  a license file is reference only.

## License

GPL-3.0-or-later. See `LICENSE` and `CREDITS.md`. Not affiliated with Telltale Games, Mojang or
Microsoft.
