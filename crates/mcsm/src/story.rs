//! First dialogue-runtime slice: authored starts, scripts, exchanges, idles and scene changes.
//! All dialogue, voice and timeline data comes from the mounted install. Unsupported graph
//! links/scripts stop with a diagnostic instead of choosing a branch on the player's behalf.
use crate::{audio, chore, data::GameData, menu, stage};
use bevy::prelude::*;
use std::{collections::HashMap, sync::Arc};
use ttmeta::{dialog::{Dialog, NodeKind}, text::LanguageDb, Scene, Symbol, Value};

#[derive(Component)]
pub struct StoryOwned;

/// The subtitle line (centered near the bottom) and the response list above it.
#[derive(Component)]
pub struct SubtitleText;
#[derive(Component)]
pub struct ChoiceText;

/// Subtitle font size: the game's Subtitle Size (0, 0.5, 1: small, medium, large) at a
/// 720-pixel-high window, scaled with the window.
fn subtitle_px(size: f32, window_height: f32) -> f32 {
    (24.0 + 12.0 * size.clamp(0.0, 1.0)) * (window_height / 720.0).max(0.5)
}

/// A centered text line anchored `bottom` percent above the window's bottom edge.
fn spawn_caption(commands: &mut Commands, bottom: f32, marker: impl Component) -> Entity {
    commands
        .spawn((
            StoryOwned,
            Node { position_type: PositionType::Absolute, left: percent(10), right: percent(10), bottom: percent(bottom), justify_content: JustifyContent::Center, ..default() },
            GlobalZIndex(i32::MAX - 2),
        ))
        .with_children(|p| {
            p.spawn((
                marker,
                Text::new(""),
                TextFont { font_size: bevy::text::FontSize::Px(28.0), ..default() },
                TextLayout::justify(Justify::Center),
                TextShadow { offset: Vec2::splat(2.0), color: Color::srgba(0.0, 0.0, 0.0, 0.85) },
            ));
        })
        .id()
}

struct Cue {
    start: f32,
    end: f32,
    text: String,
    sound: Option<Handle<AudioSource>>,
    fired: bool,
}

struct PendingChoice {
    options: Vec<(String, Symbol)>,
    post: Vec<Symbol>,
    resume: Symbol,
}

#[derive(Resource)]
pub struct Story {
    location: String,
    dialog: Option<Dialog>,
    language: LanguageDb,
    agents: Arc<HashMap<String, Entity>>,
    pub camera: Option<Entity>,
    roots: Vec<Entity>,
    next: Symbol,
    load_on_end: Option<String>,
    player: Option<Entity>,
    idle: Option<Entity>,
    cues: Vec<Cue>,
    lua: mlua::Lua,
    stopped: bool,
    pending_location: Option<String>,
    choice: Option<PendingChoice>,
    returns: Vec<Symbol>,
    choice_resume: Option<Symbol>,
    voices: Vec<Entity>,
    prepared_choice: Option<Symbol>,
    logic_initialized: bool,
    /// A script paused in `Wait` / `Yield` / `RenderDelay`, and the seconds left to wait.
    thread: Option<mlua::Thread>,
    wait: f32,
}

impl Story {
    pub fn new() -> Result<Self, String> {
        let lua = mlua::Lua::new();
        // No filesystem, process execution, dynamic native libraries, or debug access.
        for name in ["io", "os", "package", "debug", "dofile", "loadfile", "require"] {
            lua.globals().set(name, mlua::Value::Nil).map_err(|e| e.to_string())?;
        }
        let checkpoint = lua.create_function(|lua, args: mlua::MultiValue| {
            if let Some(mlua::Value::String(node)) = args.front() {
                lua.globals().set("mcsm_checkpoint", node.clone())?;
            }
            Ok(())
        }).map_err(|e| e.to_string())?;
        lua.globals().set("Checkpoint", checkpoint).map_err(|e| e.to_string())?;
        let notifications = lua.create_table().map_err(|e| e.to_string())?;
        for name in ["Notification_ShowTutorial", "Notification_ShowChoice"] {
            let function = lua.create_function(|_, args: mlua::MultiValue| {
                info!("story notification: {args:?}");
                Ok(())
            }).map_err(|e| e.to_string())?;
            lua.globals().set(name, function).map_err(|e| e.to_string())?;
        }
        lua.globals().set("mcsm_notifications", notifications).map_err(|e| e.to_string())?;
        lua.globals().set("mcsm_logic", lua.create_table().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let set = lua.create_function(|lua, (key, value): (String, mlua::Value)| {
            lua.globals().get::<mlua::Table>("mcsm_logic")?.set(format!("{:016x}", ttmeta::crc64(&key)), value)
        }).map_err(|e| e.to_string())?;
        let get = lua.create_function(|lua, key: String| {
            lua.globals().get::<mlua::Table>("mcsm_logic")?.get::<mlua::Value>(format!("{:016x}", ttmeta::crc64(&key)))
        }).map_err(|e| e.to_string())?;
        lua.globals().set("LogicSet", set).map_err(|e| e.to_string())?;
        let property = lua.create_function(|lua, (scope, key): (String, String)| {
            lua.globals().get::<mlua::Table>("mcsm_logic")?.get::<mlua::Value>(logic_key(&scope, Symbol::of(&key)))
        }).map_err(|e| e.to_string())?;
        lua.globals().set("mcsm_property", property).map_err(|e| e.to_string())?;
        lua.globals().set("LogicGet", get).map_err(|e| e.to_string())?;
        // Precompiled chunks can break out of the VM: installed scripts are source text only.
        // Scripts run as coroutines, so the engine's waits suspend them between frames; camera
        // and navigation requests are read back by `tick`.
        lua.load(
            r#"
            local text_load = load
            load = function(chunk, name, _, env) return text_load(chunk, name, "t", env) end
            loadstring = nil
            function Wait(seconds) coroutine.yield(tonumber(seconds) or 0) end
            function Sleep(seconds) coroutine.yield(tonumber(seconds) or 0) end
            function Yield() coroutine.yield(0) end
            function RenderDelay(frames) for _ = 1, (tonumber(frames) or 1) do coroutine.yield(0) end end
            function CameraActivate(name) mcsm_camera = name end
            -- What the dialogue's conditions ask about this build: the PC game, with a mouse.
            for _, name in ipairs({ "IsPlatformIOS", "IsPlatformAppleTV", "IsPlatformSmallScreen",
                    "IsPlatformXbox360", "IsPlatformXboxOne", "IsPlatformPS3", "IsPlatformPS4",
                    "IsPlatformVita", "IsPlatformWiiU", "IsPlatformMac", "Input_UseTouch" }) do
                _G[name] = function() return false end
            end
            function IsPlatformPC() return true end
            function Input_UseCursor() return true end
            function PlayerChoice_IsFemale() return mcsm_female == true end
            function AgentGetProperty(scope, name) return mcsm_property(scope, name) end
            function Navigate_Enable(on) mcsm_navigate = on end
            "#,
        )
        .set_name("mcsm prelude")
        .exec()
        .map_err(|e| e.to_string())?;
        Ok(Self {
            location: String::new(), dialog: None, language: LanguageDb::default(),
            agents: Arc::new(HashMap::new()), camera: None, roots: Vec::new(),
            next: Symbol::NONE, load_on_end: None, player: None, idle: None,
            cues: Vec::new(), lua, stopped: false, pending_location: Some("TheEnd".into()),
            choice: None, returns: Vec::new(), choice_resume: None, voices: Vec::new(), prepared_choice: None, logic_initialized: false,
            thread: None, wait: 0.0,
        })
    }

    /// Start an installed dialogue script. It runs until it finishes or waits; a waiting
    /// script is resumed by [`Story::resume`]. Each run is bounded by an instruction budget.
    fn script(&mut self, script: &str) -> Result<(), String> {
        let function = self.lua.load(script).set_name("installed dialogue script").into_function().map_err(|e| e.to_string())?;
        self.thread = Some(self.lua.create_thread(function).map_err(|e| e.to_string())?);
        self.wait = 0.0;
        self.resume()
    }

    /// A dialogue condition's Lua expression, with the same instruction budget as scripts.
    fn condition(&self, expression: &str) -> Result<bool, String> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let budget = Arc::new(AtomicUsize::new(0));
        self.lua
            .set_hook(mlua::HookTriggers::new().every_nth_instruction(1000), move |_, _| {
                if budget.fetch_add(1000, Ordering::Relaxed) >= 100_000 {
                    return Err(mlua::Error::RuntimeError("condition instruction budget exceeded".into()));
                }
                Ok(mlua::VmState::Continue)
            })
            .map_err(|e| e.to_string())?;
        let result = self.lua.load(format!("return ({expression})")).set_name("installed dialogue condition").eval::<mlua::Value>();
        self.lua.remove_hook();
        match result.map_err(|e| explain_lua(&e.to_string()))? {
            mlua::Value::Nil | mlua::Value::Boolean(false) => Ok(false),
            _ => Ok(true),
        }
    }

    /// Run the waiting script until it waits again or finishes.
    fn resume(&mut self) -> Result<(), String> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let Some(thread) = self.thread.clone() else { return Ok(()) };
        let budget = Arc::new(AtomicUsize::new(0));
        thread
            .set_hook(mlua::HookTriggers::new().every_nth_instruction(1000), move |_, _| {
                if budget.fetch_add(1000, Ordering::Relaxed) >= 100_000 {
                    return Err(mlua::Error::RuntimeError("story script instruction budget exceeded".into()));
                }
                Ok(mlua::VmState::Continue)
            })
            .map_err(|e| e.to_string())?;
        let result = thread.resume::<mlua::Value>(()).map_err(|e| explain_lua(&e.to_string()));
        match result {
            Ok(waited) if thread.status() == mlua::ThreadStatus::Resumable => {
                self.wait = match waited {
                    mlua::Value::Number(n) => n as f32,
                    mlua::Value::Integer(n) => n as f32,
                    _ => 0.0,
                };
                Ok(())
            }
            Ok(_) => {
                self.thread = None;
                Ok(())
            }
            Err(e) => {
                self.thread = None;
                Err(e)
            }
        }
    }

    fn clear(&mut self, commands: &mut Commands) {
        for e in self.roots.drain(..) { commands.entity(e).try_despawn(); }
        self.player = None;
        self.idle = None;
        self.cues.clear();
        self.camera = None;
        self.choice = None;
        self.returns.clear();
        self.choice_resume = None;
        self.voices.clear();
        self.prepared_choice = None;
        self.thread = None;
        self.wait = 0.0;
    }

    fn load(&mut self, ctx: &mut stage::Ctx, location: &str) -> Result<(), String> {
        if !self.logic_initialized {
            let table = self.lua.globals().get::<mlua::Table>("mcsm_logic").map_err(|e| e.to_string())?;
            let mut pending = vec![Symbol::of("logic_game.prop")];
            let mut seen = std::collections::HashSet::new();
            while let Some(resource) = pending.pop() {
                if !seen.insert(resource) { continue; }
                let Some(props) = ctx.props.file(ctx.data, resource) else { continue };
                pending.extend(props.parents.iter().rev().copied());
                for (key, _, value) in &props.values {
                    let key = format!("{:016x}", key.0);
                    if table.contains_key(key.clone()).map_err(|e| e.to_string())? { continue; }
                    let result = match value {
                        Value::Bool(v) => table.set(key, *v), Value::Int(v) => table.set(key, *v),
                        Value::Float(v) => table.set(key, *v), Value::String(v) => table.set(key, v.clone()),
                        _ => continue,
                    };
                    result.map_err(|e| e.to_string())?;
                }
            }
            self.logic_initialized = true;
        }
        let (scene_name, dialog_name, language_name) = match location {
            "TheEnd" => ("adv_theEnd.scene", "env_theEnd.dlog", "env_theend_english.landb"),
            "ForestTreehouse" => ("adv_forestTreehouse.scene", "env_forestTreehouse.dlog", "env_foresttreehouse_english.landb"),
            _ => return Err(format!("scene transition to {location:?} is not supported yet")),
        };
        let scene = Scene::parse(&ctx.data.read(scene_name).ok_or_else(|| format!("missing {scene_name}"))?).map_err(|e| e.to_string())?;
        let props = scene.agents.iter().find(|a| a.name == scene.name).ok_or("missing scene agent")?;
        let start = ctx.props.string(ctx.data, &props.props, "Scene - Dialog Node").ok_or("missing scene start node")?;
        let dialog = Dialog::parse(&ctx.data.read(dialog_name).ok_or_else(|| format!("missing {dialog_name}"))?).map_err(|e| e.to_string())?;
        let next = dialog.named(&start).ok_or_else(|| format!("unsupported start {start:?}"))?.id;
        let language = ctx.data.read(language_name).ok_or_else(|| format!("missing {language_name}"))?;
        let camera_name = ctx.props.string(ctx.data, &props.props, "Scene - Camera").filter(|s| !s.is_empty()).unwrap_or_else(|| "cam_cutsceneChore".into());
        self.clear(ctx.commands);
        let spawned = stage::spawn_scene_with_camera(ctx, &scene, 6, 0, stage::Shading::World, |_| false, Some(&camera_name));
        self.camera = spawned.camera;
        if self.camera.is_none() { return Err(format!("missing camera {camera_name}")); }
        self.roots.extend(spawned.agents.values().copied());
        self.agents = Arc::new(spawned.agents);
        self.dialog = Some(dialog);
        self.language = LanguageDb::parse(&language);
        self.location = location.into();
        self.next = next;
        self.load_on_end = None;
        self.stopped = false;
        // Scene-wide setup chores (ambient movement, default prop state).
        if let Some(resource) = ctx.props.symbol(ctx.data, &props.props, "Scene - Chore") {
            if let Some(name) = ctx.data.name_of(resource) {
                if let Some(c) = chore::chore_by_name(ctx.data, name) {
                    let e = ctx.commands.spawn((chore::ChorePlayer::new(ctx.data, c, self.agents.clone(), true), StoryOwned)).id();
                    self.roots.push(e);
                }
            }
        }
        let subtitle = spawn_caption(ctx.commands, 6.0, SubtitleText);
        let choices = spawn_caption(ctx.commands, 16.0, ChoiceText);
        self.roots.extend([subtitle, choices]);
        info!("story: {location}, start {start}, {} indexed nodes", self.dialog.as_ref().unwrap().nodes.len());
        Ok(())
    }

    fn start_chore(&mut self, commands: &mut Commands, data: &GameData, resource: Symbol, looping: bool) -> Result<Entity, String> {
        let name = data.name_of(resource).ok_or_else(|| format!("unknown chore {resource:?}"))?;
        let c = chore::chore_by_name(data, name).ok_or_else(|| format!("could not decode {name}"))?;
        info!("story chore: {name} ({:.2}s)", c.length);
        let mut player = chore::ChorePlayer::new(data, c, self.agents.clone(), looping);
        // Developer-only accelerated graph traversal (audio is not time-stretched).
        player.speed = std::env::var("MCSM_STORY_SPEED").ok().and_then(|v| v.parse::<f32>().ok()).filter(|v| v.is_finite() && *v > 0.0).unwrap_or(1.0);
        let e = commands.spawn((player, StoryOwned)).id();
        self.roots.push(e);
        Ok(e)
    }

    fn exchange(&mut self, commands: &mut Commands, data: &GameData, sources: &mut Assets<AudioSource>, resource: Symbol) -> Result<(), String> {
        let name = data.name_of(resource).ok_or_else(|| format!("unknown exchange {resource:?}"))?;
        let c = chore::chore_by_name(data, name).ok_or_else(|| format!("could not decode {name}"))?;
        let ids: HashMap<Symbol, u32> = self.language.texts.keys().map(|id| (Symbol::of(&format!("{id}.lang")), *id)).collect();
        self.cues.clear();
        for r in c.resources.iter().filter(|r| r.enabled) {
            let Some(id) = ids.get(&r.name) else { continue };
            let sound = data.read(&format!("{id}.wav")).and_then(|b| {
                audio::voice_stream(&b, *id).map_err(|e| warn!("voice {id}: {e}")).ok()
            }).map(|b| sources.add(AudioSource { bytes: b.into() }));
            for block in &r.blocks {
                self.cues.push(Cue { start: block.start, end: block.end, text: clean_line(self.language.get(*id).unwrap_or_default()), sound: sound.clone(), fired: false });
            }
        }
        self.cues.sort_by(|a, b| a.start.total_cmp(&b.start));
        info!("story: {} timed subtitle cues, {} decoded voice cues", self.cues.len(), self.cues.iter().filter(|c| c.sound.is_some()).count());
        self.player = Some(self.start_chore(commands, data, resource, false)?);
        Ok(())
    }
}

/// Where a logic value lives in `mcsm_logic`: logic sets (`logic_game`, `logic_inventory`)
/// share one namespace keyed by the property's symbol; agent properties are prefixed with the
/// agent's name.
fn logic_key(scope: &str, key: Symbol) -> String {
    if scope.to_ascii_lowercase().starts_with("logic_") {
        format!("{:016x}", key.0)
    } else {
        format!("{}:{:016x}", scope.to_ascii_lowercase(), key.0)
    }
}

/// Name the missing engine function in a Lua error ("attempt to call global 'X'").
fn explain_lua(error: &str) -> String {
    match error.split("attempt to call global '").nth(1).and_then(|r| r.split('\'').next()) {
        Some(name) => format!("the game's {name}() is not implemented yet"),
        None => error.to_owned(),
    }
}

/// Subtitle text without the recording markup it carries: take markers ("[pickup1]",
/// "[alt]") and emotion / gesture cues ("{SternA}", "{body-StandA}").
pub fn clean_line(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth = 0u32;
    for c in text.chars() {
        match c {
            '[' | '{' => depth += 1,
            ']' | '}' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    // Typographic quotes the UI font has no glyphs for.
    let out = out.replace(['\u{2018}', '\u{2019}'], "'").replace(['\u{201C}', '\u{201D}'], "\"").replace('\u{2026}', "...");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

const RUNTIME_VISIBLE: Symbol = Symbol::of("Runtime: Visible");
const WALK_IDLE: Symbol = Symbol::of("Walk Animation - Idle");

impl Story {
    /// Act on what the last script run asked for: `CameraActivate(name)` moves the scene
    /// camera onto that camera agent; `Navigate_Enable(true)` hands control to the player,
    /// which this runtime cannot do yet, so the story stops there with that reason.
    fn apply_requests(&mut self, commands: &mut Commands, assets: &mut menu::SceneAssets, data: &GameData, agents: &Query<&stage::TtAgent>) -> Result<(), String> {
        let globals = self.lua.globals();
        if let Ok(name) = globals.get::<String>("mcsm_camera") {
            globals.set("mcsm_camera", mlua::Value::Nil).map_err(|e| e.to_string())?;
            let target = self.agents.iter().find(|(n, _)| n.eq_ignore_ascii_case(&name)).map(|(_, e)| *e);
            match (target, self.camera) {
                (Some(agent), Some(lens)) => {
                    commands.entity(lens).insert(ChildOf(agent));
                    let ctx = assets.ctx(commands, data);
                    if let Some(fov) = agents.get(agent).ok().and_then(|a| ctx.props.float(data, &a.props, "Field of View")) {
                        ctx.commands.entity(lens).insert(stage::HorizontalFov(fov.to_radians()));
                    }
                    info!("story: camera {name}");
                }
                _ => warn!("story: CameraActivate({name:?}): no such camera in {}", self.location),
            }
        }
        if globals.get::<bool>("mcsm_navigate").unwrap_or(false) {
            globals.set("mcsm_navigate", mlua::Value::Nil).map_err(|e| e.to_string())?;
            return Err("free walking (Navigate_Enable) is not implemented yet".into());
        }
        Ok(())
    }

    /// A logic node's assignments to one scope: recorded for later conditions, and for scene
    /// agents applied where this runtime can show them (visibility, idle). Other agent
    /// properties (e.g. "Game Selectable", for navigation not implemented yet) are recorded only.
    fn assign(&mut self, commands: &mut Commands, scope: &str, props: ttmeta::PropertySet) -> Result<(), String> {
        let table = self.lua.globals().get::<mlua::Table>("mcsm_logic").map_err(|e| e.to_string())?;
        let agent = (!scope.to_ascii_lowercase().starts_with("logic_"))
            .then(|| self.agents.iter().find(|(name, _)| name.eq_ignore_ascii_case(scope)).map(|(_, e)| *e))
            .flatten();
        if agent.is_none() && !scope.to_ascii_lowercase().starts_with("logic_") {
            warn!("story: logic assignment to {scope:?}, which is not in {}", self.location);
        }
        for (key, _, value) in props.values {
            let k = logic_key(scope, key);
            let result = match &value {
                Value::Bool(v) => table.set(k, *v),
                Value::Int(v) => table.set(k, *v),
                Value::Float(v) => table.set(k, *v),
                Value::String(v) => table.set(k, v.clone()),
                Value::Symbol(v) => table.set(k, format!("{:016x}", v.0)),
                Value::AnimOrChore(a, c) => table.set(k, format!("{:016x}/{:016x}", a.0, c.0)),
                other => return Err(format!("unsupported assignment value {other:?} for {scope}")),
            };
            result.map_err(|e| e.to_string())?;
            let Some(e) = agent else { continue };
            match (key, value) {
                (RUNTIME_VISIBLE, Value::Bool(v)) => {
                    commands.entity(e).insert(if v { Visibility::Inherited } else { Visibility::Hidden });
                }
                (WALK_IDLE, Value::AnimOrChore(a, c)) => {
                    commands.entity(e).insert(crate::character::SetIdle(a, c));
                }
                _ => {}
            }
        }
        Ok(())
    }
}

pub fn tick(
    mut commands: Commands,
    data: Option<Res<GameData>>,
    mut menu: ResMut<menu::MenuState>,
    mut story: Option<ResMut<Story>>,
    mut assets: menu::SceneAssets,
    mut sources: ResMut<Assets<AudioSource>>,
    players: Query<&chore::ChorePlayer>,
    mut status: Query<&mut Text, With<menu::StatusLine>>,
    settings: Option<Res<crate::settings::Settings>>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    fovs: Query<&stage::TtAgent>,
    mut captions: Query<(&mut Text, &mut TextFont, Has<ChoiceText>), (Or<(With<SubtitleText>, With<ChoiceText>)>, Without<menu::StatusLine>)>,
    windows: Query<&Window>,
) {
    let (Some(data), Some(story)) = (data, story.as_mut()) else { return };
    if !menu.in_story() {
        story.clear(&mut commands);
        commands.remove_resource::<Story>();
        return;
    }
    let result = (|| -> Result<(), String> {
        if let Some(location) = story.pending_location.take() {
            story.load(&mut assets.ctx(&mut commands, &data), &location)?;
        }
        menu.set_story_camera(story.camera);
        if story.stopped { return Ok(()); }
        if story.choice.is_some() {
            let selected = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4].iter().position(|key| keys.just_pressed(*key)).or_else(|| {
                // Explicit developer automation: choose only once the prompt has finished.
                let finished = story.player.and_then(|e| players.get(e).ok()).is_some_and(|p| p.time >= p.chore.length);
                if !finished { return None; }
                std::env::var("MCSM_STORY_AUTO_CHOICE").ok().and_then(|v| v.parse::<usize>().ok()).filter(|i| (1..=4).contains(i)).map(|i| i - 1)
            });
            if let Some(index) = selected {
                let choice = story.choice.take().unwrap();
                if let Some((label, target)) = choice.options.get(index) {
                    info!("story choice {}: {label}", index + 1);
                    if let Some(e) = story.player.take() { commands.entity(e).try_despawn(); }
                    story.cues.clear();
                    for e in story.voices.drain(..) { commands.entity(e).try_despawn(); }
                    story.returns.push(choice.resume);
                    story.returns.extend(choice.post.iter().rev().copied());
                    story.choice_resume = Some(choice.resume);
                    story.next = *target;
                } else { story.choice = Some(choice); }
            }
        }
        if let Some(player) = story.player {
            let Ok(player) = players.get(player) else { return Ok(()) };
            let t = player.time;
            let mut subtitle = String::new();
            let mut spawned_audio = Vec::new();
            for cue in &mut story.cues {
                if t >= cue.start && t < cue.end {
                    subtitle = cue.text.clone();
                    if !cue.fired {
                        cue.fired = true;
                        if let Some(sound) = &cue.sound {
                            let master = settings.as_ref().map_or(1.0, |s| match s.applied(crate::settings::VOLUME_MASTER) { crate::settings::SettingValue::Float(v) => v, _ => 1.0 });
                            let e = commands.spawn((AudioPlayer(sound.clone()), PlaybackSettings::DESPAWN.with_volume(bevy::audio::Volume::Linear(master)), StoryOwned)).id();
                            spawned_audio.push(e);
                        }
                    }
                }
            }
            story.voices.extend(spawned_audio.iter().copied());
            story.roots.extend(spawned_audio);
            for mut text in &mut status {
                text.0.clear();
            }
            let subtitles = settings.as_ref().is_none_or(|s| s.applied(crate::settings::SUBTITLES) != crate::settings::SettingValue::Bool(false));
            let size = settings.as_ref().map_or(0.5, |s| match s.applied(crate::settings::SUBTITLE_SIZE) {
                crate::settings::SettingValue::Float(v) => v,
                _ => 0.5,
            });
            let px = subtitle_px(size, windows.iter().next().map_or(720.0, |w| w.height()));
            for (mut text, mut font, is_choice) in &mut captions {
                font.font_size = bevy::text::FontSize::Px(px);
                text.0 = if is_choice {
                    story.choice.as_ref().map_or(String::new(), |c| {
                        c.options.iter().enumerate().map(|(i, (label, _))| format!("{}.  {label}", i + 1)).collect::<Vec<_>>().join("\n")
                    })
                } else if subtitles {
                    subtitle.clone()
                } else {
                    String::new()
                };
            }
            if t < player.chore.length || story.choice.is_some() { return Ok(()); }
            commands.entity(story.player.take().unwrap()).try_despawn();
        }
        story.lua.globals().set("mcsm_female", menu.player_is_female()).map_err(|e| e.to_string())?;
        // A script waiting in Wait / Yield resumes when its time is up.
        if story.thread.is_some() {
            let speed = std::env::var("MCSM_STORY_SPEED").ok().and_then(|v| v.parse::<f32>().ok()).filter(|v| v.is_finite() && *v > 0.0).unwrap_or(1.0);
            story.wait -= time.delta_secs().min(0.1) * speed;
            if story.wait > 0.0 {
                return Ok(());
            }
            story.resume()?;
            story.apply_requests(&mut commands, &mut assets, &data, &fovs)?;
            if story.thread.is_some() {
                return Ok(());
            }
        }
        // A bounded batch: script-only cycles cannot freeze a frame.
        for _ in 0..64 {
            let node = story.dialog.as_ref().and_then(|d| d.nodes.get(&story.next)).cloned();
            let Some(node) = node else {
                if let Some(next) = story.returns.pop() {
                    story.next = next;
                    continue;
                }
                if let Some(location) = story.load_on_end.take() {
                    // The current graph's chain exits to its folder. The opening graph has
                    // a declared scene-on-end; do not synthesize transitions without one.
                    story.pending_location = Some(location);
                    return Ok(());
                }
                // The chain ran out with nowhere to return to: the scene now hands control to
                // the player, and the story continues through what they walk to and use.
                return Err(format!("this is where you walk around freely in {}, which is not implemented yet", story.location));
            };
            story.next = node.next;
            match node.kind {
                NodeKind::Start => {
                    if let Some(Value::String(location)) = node.props.get_str("Game - Load Script On End") {
                        if !location.is_empty() { story.load_on_end = Some(location.clone()); }
                    }
                }
                NodeKind::Script(ref text) => {
                    story.script(text)?;
                    story.apply_requests(&mut commands, &mut assets, &data, &fovs)?;
                    if story.thread.is_some() {
                        return Ok(());
                    }
                }
                NodeKind::Idle(resource) => {
                    if let Some(old) = story.idle.take() { commands.entity(old).try_despawn(); }
                    if resource != Symbol::NONE { story.idle = Some(story.start_chore(&mut commands, &data, resource, true)?); }
                }
                NodeKind::Assign(groups) => {
                    for (scope, props) in groups {
                        story.assign(&mut commands, &scope, props)?;
                    }
                }
                NodeKind::Conditional(cases) => {
                    let mut target = None;
                    let table = story.lua.globals().get::<mlua::Table>("mcsm_logic").map_err(|e| e.to_string())?;
                    for case in cases {
                        let mut visible = true;
                        for (scope, key, expected, negate) in case.condition {
                            let actual = table.get::<mlua::Value>(logic_key(&scope, key)).map_err(|e| e.to_string())?;
                            let equal = match (actual, expected) {
                                (mlua::Value::Boolean(a), Value::Bool(b)) => a == b,
                                (mlua::Value::Integer(a), Value::Int(b)) => a == b as i64,
                                (mlua::Value::Number(a), Value::Int(b)) => a == b as f64,
                                (mlua::Value::Number(a), Value::Float(b)) => a == b as f64,
                                (mlua::Value::Integer(a), Value::Float(b)) => a as f64 == b as f64,
                                (mlua::Value::String(a), Value::String(b)) => a.to_str().map_err(|e| e.to_string())? == b,
                                (mlua::Value::Nil, _) => return Err(format!("missing condition key {scope}:{key:?}")),
                                _ => return Err("unsupported conditional value type".into()),
                            };
                            visible &= equal != negate;
                        }
                        if visible && !case.script.is_empty() {
                            visible = story.condition(&case.script)?;
                        }
                        if visible { target = Some(case.target); break; }
                    }
                    if let Some(target) = target {
                        story.returns.push(node.next);
                        story.next = target;
                    }
                }
                NodeKind::UnsupportedDetail(detail) => return Err(detail),
                NodeKind::Unsupported(kind) => return Err(format!("unsupported {kind} node {:?}", node.id)),
                NodeKind::CancelChoices => { story.choice = None; }
                NodeKind::PassThrough => {}
                NodeKind::Jump { target, name, target_class, behavior } => {
                    if target_class == 3 && behavior == 1 {
                        story.next = story.choice_resume.take().ok_or("jump without a parent choice wait")?;
                        story.returns.clear();
                    } else if target_class == 1 && behavior == 1 {
                        story.next = if story.dialog.as_ref().unwrap().nodes.contains_key(&target) { target }
                            else { story.dialog.as_ref().unwrap().nodes.values().find(|n| n.name == name).ok_or("missing jump target")?.id };
                    } else { return Err(format!("unsupported jump class {target_class}, behavior {behavior}")); }
                }
                NodeKind::Choices { choices, before, after } => {
                    let before: Vec<_> = before.into_iter().filter(|id| story.dialog.as_ref().unwrap().nodes.contains_key(id)).collect();
                    if !before.is_empty() && story.prepared_choice != Some(node.id) {
                        story.prepared_choice = Some(node.id);
                        story.returns.push(node.id);
                        story.returns.extend(before.iter().skip(1).rev().copied());
                        story.next = before[0];
                        continue;
                    }
                    story.prepared_choice = None;
                    let dialog = story.dialog.as_ref().unwrap();
                    let mut prompt = node.next;
                    let mut resume = None;
                    for _ in 0..64 {
                        let n = dialog.nodes.get(&prompt).ok_or("missing choice prompt chain")?;
                        if matches!(n.kind, NodeKind::Unsupported("wait")) { resume = Some(n.next); break; }
                        prompt = n.next;
                    }
                    let resume = resume.ok_or("missing parent choice wait")?;
                    let mut options = Vec::new();
                    for choice in choices {
                        let lang = choice.props.values.iter().find_map(|(_, ty, v)| {
                            if *ty == ttmeta::crc64("LanguageResProxy") { if let Value::Int(id) = v { return Some(*id as u32); } }
                            None
                        });
                        let label = lang.and_then(|id| story.language.get(id)).map_or_else(|| "...".to_owned(), clean_line);
                        options.push((label, choice.target));
                    }
                    story.choice = Some(PendingChoice { options, post: after.into_iter().filter(|id| dialog.nodes.contains_key(id)).collect(), resume });
                    // The prompt exchange runs while the response buttons are available.
                    if let Some(n) = story.dialog.as_ref().unwrap().nodes.get(&node.next) {
                        if let NodeKind::Exchange(resource) = n.kind {
                            story.exchange(&mut commands, &data, &mut sources, resource)?;
                            return Ok(());
                        }
                    }
                    return Err("unsupported choice prompt".into());
                }
                NodeKind::Exchange(resource) => {
                    if let Some(old) = story.idle.take() { commands.entity(old).try_despawn(); }
                    story.exchange(&mut commands, &data, &mut sources, resource)?;
                    return Ok(());
                }
                NodeKind::Chore { resource, looping } => {
                    if looping { return Err("blocking looped dialogue chore is not supported".into()); }
                    story.player = Some(story.start_chore(&mut commands, &data, resource, false)?);
                    return Ok(());
                }
            }
        }
        Err("dialogue execution budget exceeded".into())
    })();
    if let Err(error) = result {
        error!("story stopped: {error}");
        story.stopped = true;
        for mut text in &mut status { text.0 = format!("Story stopped: {error}. Esc returns to the menu."); }
        for (mut text, _, _) in &mut captions { text.0.clear(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scripts_fail_loudly_and_have_a_budget() {
        let mut story = Story::new().unwrap();
        assert!(story.script("MissingEngineFunction()").is_err());
        assert!(story.script("while true do end").is_err());
        assert!(story.script("Checkpoint('test')").is_ok());
        assert!(story.script("LogicSet('test_key', 3); assert(LogicGet('TEST_KEY') == 3)").is_ok());
        assert!(story.script("os.execute('anything')").is_err());
        // Waits suspend the script; precompiled chunks are refused.
        assert!(story.script("Wait(2); LogicSet('after', 1)").is_ok());
        assert!(story.thread.is_some() && story.wait == 2.0);
        assert!(story.resume().is_ok() && story.thread.is_none());
        assert!(story.script("assert(load(string.dump(function() end)) == nil)").is_ok());
        assert_eq!(explain_lua("x: attempt to call global 'Struggle_Enter' (a nil value)"), "the game's Struggle_Enter() is not implemented yet");
        // Dialogue conditions.
        story.lua.globals().set("mcsm_female", true).unwrap();
        assert_eq!(story.condition("not PlayerChoice_IsFemale()"), Ok(false));
        assert_eq!(story.condition("IsPlatformPC() and Input_UseCursor() and not IsPlatformIOS()"), Ok(true));
        assert!(story.script("LogicSet('Apples', 2)").is_ok());
        assert_eq!(story.condition("AgentGetProperty('logic_game', 'Apples') == 2"), Ok(true));
        assert!(story.condition("Crafting_IsItemBuilt({})").unwrap_err().contains("Crafting_IsItemBuilt"));
    }

    #[test]
    fn subtitle_markup_is_removed() {
        assert_eq!(clean_line("[take2]{MoodA}{body-PoseB}The quick {MoodB}brown fox,"), "The quick brown fox,");
        assert_eq!(clean_line("[alt] Jumps over"), "Jumps over");
        assert_eq!(clean_line("It\u{2019}s lazy\u{2026}"), "It's lazy...");
        assert_eq!(subtitle_px(0.5, 720.0), 30.0);
    }
}
