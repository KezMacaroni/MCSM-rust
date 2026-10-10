//! The main menu and its settings menus, rebuilt from the game's own data.
//!
//! Behaviour reproduced from the game's scripts (read for reference, not copied):
//! - `Menu_Main`: the key-art scene behind `ui_menuMain.scene`; a vertical, left-aligned list
//!   holding the title and, for a licensed PC copy not running under Steam: Play, Episodes,
//!   Save Files, My Choices (stats), Settings, Exit Game.
//! - `Menu_Options` and its submenus (`Menu_AV`, `Menu_Gameplay`, `Menu_Graphics`): list menus
//!   with a header, list buttons / toggles / sliders and a tooltip line; `Options_ExitSettings`
//!   asks to Apply or Restore when settings were changed (`UI_Confirm`, `ui_popup.scene`).
//! - `ListMenu:Layout`: widgets stack downward from the menu's "Extents Max".y; each is placed
//!   so its "Extents Min".x meets the menu's, and the next starts at its own "Extents Min".y.
//!   `Header:Place` puts the header just above that top edge.
//! - `ListToggle` cycles through its values ("label : value"); `ListSlider` (0..1, step 0.1,
//!   shown as tenths) moves its handle between the bar's ends; the resolution slider indexes
//!   the display's resolutions.
//! - `Menu_Text(label)`: a label's text is the dialog node `label` in `ui_menu.dlog`.
//!
//! Not yet reproduced: show/hide transition chores, the news feed, labels behind dialog
//! conditionals (Episodes, Controls), the Controls / Credits / account screens, and the
//! actions of Play, Save Files and My Choices.

use crate::chore::{AgentMaterials, ChorePlayer, DespawnWhenDone};
use crate::data::{AssetCache, GameData, Props};
use crate::settings::{self, SettingValue, Settings};
use crate::stage::{self, Ctx, Shading};
use crate::text::{self, TextStyle};
use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use ttmeta::{PropertySet, Scene};

const KEY_ART_SCENE: &str = "adv_keyArt101.scene";
const KEY_ART_CHORE: &str = "ui_keyArt101.chore";
const KEY_ART_ASPECT_CHORE: &str = "ui_keyArt_aspectRatio.chore";
const LEGEND_PROP: &str = "ui_legend.prop";
const MENU_SHOW_CHORE: &str = "ui_menuMain_show.chore";
const MENU_SCENE: &str = "ui_menuMain.scene";
const MENU_PROP: &str = "ui_menu.prop";
const MENU_DIALOG: &str = "ui_menu.dlog";
const MENU_LANGUAGE: &str = "ui_menu_english.landb";
const DIORAMA_SCENE: &str = "ui_diorama.scene";
const EPISODE_SCENE: &str = "ui_episodeSelect.scene";
const EPISODE_ASPECT_CHORE: &str = "ui_episodeSelect_aspectRatio.chore";
const CHARACTER_SELECT_SCENE: &str = "ui_characterSelect.scene";
const CHARACTER_SELECT_ART: &str = "ui_characterSelect_keyArt.scene";
/// Appearance choices: 1-3 female, 4-6 male (`kFemaleChoices`, `kMaleChoices`).
const APPEARANCES: u8 = 6;
const KEY_ART_LAYER: usize = 1;
/// The character-select scene (3D, drawn instead of the key art).
const CHARACTER_LAYER: usize = 5;
/// The episode-select diorama (3D, drawn instead of the key art).
const DIORAMA_LAYER: usize = 4;
const MENU_LAYER: usize = 2;
const POPUP_LAYER: usize = 3;
/// `Legend_Add` places the legend (and the back button) at this height.
const LEGEND_Y: f32 = -4.2;
/// `Menu_Add(TooltipDisplay, "tooltipDisplay", 8, ...)`: wrap width.
const TOOLTIP_WIDTH: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Main,
    Settings,
    AudioVideo,
    Gameplay,
    Graphics,
    /// `UI_Confirm("popup_settingsconfirm_header", ...)`, over the Settings page.
    ConfirmSettings,
    /// `Menu_EpisodeSelect`: the diorama with one scene per episode, the episode's title and a
    /// status button ("Start Episode N"), and arrows to slide between episodes.
    EpisodeSelect,
    /// `Menu_CharacterSelect`: choose Jesse's look before the first episode menu.
    CharacterSelect,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Episodes,
    SaveFiles,
    Stats,
    Open(Page),
    Controls,
    Credits,
    Exit,
    Apply,
    Restore,
    StartEpisode,
    ChooseAppearance(u8),
    SlideLeft,
    SlideRight,
}

#[derive(Clone)]
enum Kind {
    Button(Action),
    Toggle { key: &'static str, label: String, values: Vec<SettingValue>, display: Vec<String> },
    Slider { key: &'static str, label: String, step: f32, resolutions: Option<Vec<(u32, u32)>> },
}

/// The game's widget templates and their chores.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Widget {
    Button,
    Toggle,
    Slider,
    Popup,
    /// The episode-select status button.
    Status,
    /// Episode-select arrows (no selection chores).
    Arrow,
    /// Character-select appearance buttons.
    Appearance,
}

#[derive(Clone, Default)]
struct WidgetChores {
    select: Option<Arc<ttmeta::Chore>>,
    deselect: Option<Arc<ttmeta::Chore>>,
    press: Option<Arc<ttmeta::Chore>>,
}

/// Where a widget's label text lives, so it can be rebuilt when its value changes.
#[derive(Clone)]
struct TextSlot {
    agent: Entity,
    anchor: Entity,
    style: TextStyle,
}

#[derive(Component)]
pub struct MenuItem {
    index: usize,
    kind: Kind,
    widget: Widget,
    /// Hit area in the widget root's space.
    extents: (Vec2, Vec2),
    agents: Arc<HashMap<String, Entity>>,
    text: Option<TextSlot>,
    tooltip: Option<String>,
    /// Slider handle: entity, x at the bar's left / right end, its y and z.
    handle: Option<(Entity, f32, f32, Vec2)>,
}

#[derive(Component)]
pub struct TooltipLine {
    anchor: Entity,
    style: TextStyle,
    shown: Option<usize>,
}

/// The clickable "Previous Menu" legend entry: (min, max) in world space on the menu plane.
#[derive(Component)]
pub struct LegendBack(Vec2, Vec2);

#[derive(Resource)]
pub struct MenuAssets {
    templates: HashMap<Widget, Scene>,
    header: Option<Scene>,
    tooltip: Option<Scene>,
    popup: Option<Scene>,
    chores: HashMap<Widget, WidgetChores>,
    dlog: Vec<u8>,
    language: ttmeta::text::LanguageDb,
    legend: Option<Arc<PropertySet>>,
    /// `ui_episode_info.prop` "Episode Count".
    episodes: u32,
    /// Height of the main page's title widget (the list's first entry there).
    title_height: f32,
    title_extents: Option<(Vec2, Vec2)>,
    /// ui_menu.prop's extents and its "Aspect Ratio - Chore", which widens them with the screen.
    base_extents: (Vec2, Vec2),
    aspect_chore: Option<ttmeta::Chore>,
    /// The aspect ratio the layout was made for.
    aspect: f32,
    menu_min: Vec2,
    menu_max: Vec2,
}

impl MenuAssets {
    /// `Menu_Text(label)`.
    fn text(&self, label: &str) -> Option<String> {
        let id = ttmeta::text::dialog_label(&self.dlog, label, &self.language)?;
        self.language.get(id).map(str::to_owned)
    }
}

#[derive(Resource, Default)]
pub struct MenuState {
    stack: Vec<Page>,
    /// Selection of each page on the stack.
    selections: Vec<usize>,
    count: usize,
    camera: Option<Entity>,
    popup_camera: Option<Entity>,
    /// The item whose select chore last played.
    shown: Option<usize>,
    rebuild: bool,
    roots: Vec<Entity>,
    title: Option<Entity>,
    dragging: Option<usize>,
    /// Items whose value changed (their label / handle needs refreshing).
    changed: Vec<usize>,
    key_art_camera: Option<Entity>,
    /// Episode shown on the episode-select page (1-based; 0 before the first visit).
    episode: u32,
    /// A slide to another episode in progress: (target episode, seconds left).
    slide: Option<(u32, f32)>,
    /// The episode's show chore is due (entering the page).
    show_pending: bool,
    /// `PlayerChoice`: the chosen look (1-6), once character select has been completed.
    player_choice: Option<u8>,
    /// The look shown on the character-select scene, and its performance chore.
    appearance_shown: Option<u8>,
    appearance_chore: Option<Entity>,
    /// The look applied to the diorama's Jesse.
    diorama_appearance: Option<u8>,
    /// A background transition (fade) in progress.
    transition: Option<Transition>,
}

/// Where a background change leads once the screen is black.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    /// Menu_Main_Play before a look is chosen.
    ToCharacterSelect,
    /// To the episode menu; `replace` drops the current page first (after character select).
    ToEpisodes { replace: bool },
    /// Menu_Pop to a page with another background.
    Back,
}

/// A fill fade: the "Render Constant Alpha" keys of one of the game's fill chores.
#[derive(Clone)]
pub struct FadeCurve {
    keys: Vec<ttmeta::anim::Key<f32>>,
    length: f32,
}

impl FadeCurve {
    fn load(data: &GameData, chore: &str) -> Option<Self> {
        let c = load_chore(data, chore)?;
        let keys = c.resources.iter().find_map(|r| match r.control.value("Render Constant Alpha") {
            Some(ttmeta::anim::Track::Float(k)) if !k.is_empty() => Some(k.clone()),
            _ => None,
        })?;
        let length = keys.last().map_or(c.length, |k| k.time).max(0.01);
        Some(FadeCurve { keys, length })
    }

    fn alpha(&self, t: f32) -> f32 {
        ttmeta::anim::sample_float(&self.keys, t).unwrap_or(0.0).clamp(0.0, 1.0)
    }
}

#[derive(Clone)]
enum FadePhase {
    /// Fading the old background out; the navigation happens at the end.
    Hide(f32),
    /// The new page is being built (one frame, black).
    Build,
    /// Fading the new background in.
    Show(f32),
}

/// A background transition (`Menu_Push` / `Menu_Pop`: old `transitionFrom`, then new
/// `transitionTo`), shown as a full-screen fade.
pub struct Transition {
    nav: Nav,
    hide: Option<FadeCurve>,
    show: Option<FadeCurve>,
    phase: FadePhase,
}

/// The full-screen black fill the transitions fade.
#[derive(Component)]
pub struct FadeOverlay;

/// The background of a page: transitions only happen when it changes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Background {
    Main,
    Characters,
    Episodes,
}

fn background(page: Page) -> Background {
    match page {
        Page::CharacterSelect => Background::Characters,
        Page::EpisodeSelect => Background::Episodes,
        _ => Background::Main,
    }
}

/// Start a background transition with the game's hide / show chores for the two backgrounds.
fn begin_transition(data: &GameData, state: &mut MenuState, nav: Nav) {
    if state.transition.is_some() {
        return;
    }
    let from = background(state.top());
    let to = match nav {
        Nav::ToCharacterSelect => Background::Characters,
        Nav::ToEpisodes { .. } => Background::Episodes,
        Nav::Back => background(state.stack.get(state.stack.len().wrapping_sub(2)).copied().unwrap_or(Page::Main)),
    };
    // bgMain / bgCharacterSelect / bgEpisodeSelect: transitionFrom, then transitionTo.
    let hide = match from {
        Background::Main => "ui_keyArt_hide.chore",
        Background::Characters => "ui_characterSelect_hide.chore",
        Background::Episodes => "ui_diorama_hide.chore",
    };
    let show = match to {
        Background::Main => "ui_keyArt_show.chore",
        Background::Characters => "ui_characterSelect_show.chore",
        Background::Episodes => "ui_diorama_show.chore",
    };
    state.transition = Some(Transition { nav, hide: FadeCurve::load(data, hide), show: FadeCurve::load(data, show), phase: FadePhase::Hide(0.0) });
}

fn apply_nav(commands: &mut Commands, state: &mut MenuState, nav: Nav) {
    match nav {
        Nav::ToCharacterSelect => {
            state.appearance_shown = None;
            state.push(Page::CharacterSelect);
        }
        Nav::ToEpisodes { replace } => {
            if replace {
                if let Some(old) = state.appearance_chore.take() {
                    commands.entity(old).try_despawn();
                }
                state.pop();
            }
            if state.episode == 0 {
                state.episode = 1;
            }
            state.show_pending = true;
            state.push(Page::EpisodeSelect);
        }
        Nav::Back => {
            if let Some(old) = state.appearance_chore.take() {
                commands.entity(old).try_despawn();
            }
            state.pop();
        }
    }
}

/// Run the fade: out, navigate while black, wait for the new page to be built, then in.
pub fn transition_tick(
    mut commands: Commands,
    time: Res<Time>,
    mut state: ResMut<MenuState>,
    mut overlay: Query<&mut BackgroundColor, With<FadeOverlay>>,
    data: Option<Res<GameData>>,
    mut debug_played: Local<bool>,
) {
    // MCSM_DEBUG_PLAY=<seconds>: press Play at that time (screenshots of the transition).
    if let (Some(at), Some(data)) = (std::env::var("MCSM_DEBUG_PLAY").ok().and_then(|v| v.parse::<f32>().ok()), data.as_ref()) {
        if !*debug_played && time.elapsed_secs() >= at && state.top() == Page::Main {
            *debug_played = true;
            let nav = if state.player_choice.is_none() { Nav::ToCharacterSelect } else { Nav::ToEpisodes { replace: false } };
            begin_transition(data, &mut state, nav);
        }
    }
    let dt = time.delta_secs().min(1.0 / 30.0);
    let mut alpha = 0.0;
    if let Some(mut tr) = state.transition.take() {
        match tr.phase {
            FadePhase::Hide(t) => {
                let t = t + dt;
                let length = tr.hide.as_ref().map_or(0.0, |c| c.length);
                alpha = tr.hide.as_ref().map_or(1.0, |c| c.alpha(t));
                if t >= length {
                    alpha = 1.0;
                    apply_nav(&mut commands, &mut state, tr.nav);
                    tr.phase = FadePhase::Build;
                } else {
                    tr.phase = FadePhase::Hide(t);
                }
                state.transition = Some(tr);
            }
            FadePhase::Build => {
                alpha = 1.0;
                // The page was rebuilt last frame (loading happens behind the black).
                if !state.rebuild {
                    tr.phase = FadePhase::Show(0.0);
                }
                state.transition = Some(tr);
            }
            FadePhase::Show(t) => {
                let t = t + dt;
                let length = tr.show.as_ref().map_or(0.0, |c| c.length);
                alpha = tr.show.as_ref().map_or(0.0, |c| c.alpha(t));
                if t < length {
                    tr.phase = FadePhase::Show(t);
                    state.transition = Some(tr);
                }
            }
        }
    }
    for mut bg in &mut overlay {
        if (bg.0.alpha() - alpha).abs() > 1e-4 {
            bg.0 = Color::srgba(0.0, 0.0, 0.0, alpha);
        }
    }
}

/// The spawned episode-select diorama: its agents (for chores) and camera.
#[derive(Resource)]
pub struct Diorama {
    agents: Arc<HashMap<String, Entity>>,
    camera: Option<Entity>,
}

impl MenuState {
    fn top(&self) -> Page {
        self.stack.last().copied().unwrap_or(Page::Main)
    }

    fn selected(&self) -> usize {
        self.selections.last().copied().unwrap_or(0)
    }

    fn select(&mut self, i: usize) {
        if let Some(s) = self.selections.last_mut() {
            *s = i;
        }
    }

    fn push(&mut self, page: Page) {
        self.stack.push(page);
        self.selections.push(0);
        self.rebuild = true;
    }

    fn pop(&mut self) {
        if self.stack.len() > 1 {
            self.stack.pop();
            self.selections.pop();
            self.rebuild = true;
        }
    }
}

#[derive(Component)]
pub struct StatusLine;

/// The asset stores a scene spawn needs, as one system parameter.
#[derive(SystemParam)]
pub struct SceneAssets<'w> {
    props: ResMut<'w, Props>,
    cache: ResMut<'w, AssetCache>,
    images: ResMut<'w, Assets<Image>>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    lit_materials: ResMut<'w, Assets<crate::lit::LitMaterial>>,
    bindposes: ResMut<'w, Assets<SkinnedMeshInverseBindposes>>,
}

impl SceneAssets<'_> {
    fn ctx<'a, 'w, 's>(&'a mut self, commands: &'a mut Commands<'w, 's>, data: &'a GameData) -> Ctx<'a, 'w, 's> {
        Ctx {
            commands,
            data,
            props: &mut self.props,
            cache: &mut self.cache,
            images: &mut self.images,
            meshes: &mut self.meshes,
            materials: &mut self.materials,
            lit_materials: &mut self.lit_materials,
            bindposes: &mut self.bindposes,
        }
    }
}

fn load_scene(data: &GameData, name: &str) -> Option<Scene> {
    let bytes = data.read(name)?;
    Scene::parse(&bytes).map_err(|e| error!("{name}: {e}")).ok()
}

fn agent_props(scene: &Scene, name: &str) -> PropertySet {
    scene.agents.iter().find(|a| a.name == name).map(|a| a.props.clone()).unwrap_or_default()
}

fn extents(props: &mut Props, data: &GameData, set: &PropertySet) -> Option<(Vec2, Vec2)> {
    let min = props.vec3(data, set, "Extents Min")?;
    let max = props.vec3(data, set, "Extents Max")?;
    Some((Vec2::new(min[0], min[1]), Vec2::new(max[0], max[1])))
}

/// The key art's and the menu's aspect-ratio chores are held at (aspect - 4/3) / (4/9) of their
/// length (AspectRatio.lua): 0 at 4:3, 1 at 16:9.
fn aspect_portion(aspect: f32) -> f32 {
    ((aspect - 4.0 / 3.0) / (4.0 / 9.0)).clamp(0.0, 1.0)
}

/// The list menu's extents at an aspect ratio: ui_menu.prop's, as keyed by its aspect chore.
fn menu_extents(base: (Vec2, Vec2), chore: Option<&ttmeta::Chore>, aspect: f32) -> (Vec2, Vec2) {
    let (mut min, mut max) = base;
    if let Some(c) = chore {
        let t = aspect_portion(aspect) * c.length;
        for res in &c.resources {
            for (key, target) in [("Extents Min", &mut min), ("Extents Max", &mut max)] {
                if let Some(ttmeta::anim::Track::Vec3(k)) = res.control.value(key) {
                    if let Some(v) = ttmeta::anim::sample_vec3(k, t) {
                        *target = Vec2::new(v[0], v[1]);
                    }
                }
            }
        }
    }
    (min, max)
}

/// The key art's aspect-ratio chore player.
#[derive(Component)]
pub struct KeyArtAspect;

/// Follow window resizes and resolution changes: re-hold the aspect chores and re-lay out the
/// menu for the new aspect ratio.
pub fn follow_aspect(
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    menu: Option<ResMut<MenuAssets>>,
    mut state: ResMut<MenuState>,
    mut players: Query<&mut ChorePlayer, With<KeyArtAspect>>,
    mut transforms: Query<&mut Transform>,
) {
    let (Ok(window), Some(mut menu)) = (windows.single(), menu) else { return };
    if window.width() < 1.0 || window.height() < 1.0 {
        return;
    }
    let aspect = window.width() / window.height();
    if (aspect - menu.aspect).abs() < 1e-3 {
        return;
    }
    menu.aspect = aspect;
    let (min, max) = menu_extents(menu.base_extents, menu.aspect_chore.as_ref(), aspect);
    menu.menu_min = min;
    menu.menu_max = max;
    if let (Some(title), Some(ext)) = (state.title, menu.title_extents) {
        if let Ok(mut tf) = transforms.get_mut(title) {
            tf.translation = Vec3::new(min.x - ext.0.x, max.y - ext.1.y, 0.0);
        }
    }
    for mut p in &mut players {
        p.time = aspect_portion(aspect) * p.chore.length;
    }
    state.rebuild = true;
}

fn load_chore(data: &GameData, name: &str) -> Option<Arc<ttmeta::Chore>> {
    crate::chore::chore_by_name(data, name)
}

#[allow(clippy::too_many_arguments)]
pub fn build(
    mut commands: Commands,
    data: Option<Res<GameData>>,
    mut assets: SceneAssets,
    mut state: ResMut<MenuState>,
    mut ambient: ResMut<GlobalAmbientLight>,
    windows: Query<&Window>,
) {
    let Some(data) = data else { return };
    let data = data.clone();
    let aspect = windows.single().map_or(16.0 / 9.0, |w| w.width() / w.height().max(1.0));
    // Lighting in the key art is baked; nothing else should light it.
    ambient.brightness = 0.0;
    ambient.affects_lightmapped_meshes = false;
    let mut ctx = assets.ctx(&mut commands, &data);

    // The key art's own fill is driven by transition chores this menu does not play yet.
    let key_art_fill = |n: &str| n.ends_with("_fillForeground");
    match load_scene(&data, KEY_ART_SCENE) {
        Some(scene) => {
            let spawned = stage::spawn_scene(&mut ctx, &scene, KEY_ART_LAYER, 0, Shading::World, key_art_fill);
            state.key_art_camera = spawned.camera;
            info!("key art: {} agents", spawned.agents.len());
            // StartKeyArt: the aspect-ratio chore is paused at a time derived from the screen's
            // aspect ratio, then the key-art chore plays once: an intro montage that settles on
            // a resting shot, which is then held.
            let agents = Arc::new(spawned.agents);
            if let Some(c) = load_chore(&data, KEY_ART_ASPECT_CHORE) {
                let mut p = ChorePlayer::new(&data, c, agents.clone(), false);
                p.time = aspect_portion(aspect) * p.chore.length;
                p.speed = 0.0;
                ctx.commands.spawn((Name::new(KEY_ART_ASPECT_CHORE), p, KeyArtAspect));
            }
            if let Some(c) = load_chore(&data, KEY_ART_CHORE) {
                ctx.commands.spawn((Name::new(KEY_ART_CHORE), ChorePlayer::new(&data, c, agents, false)));
            }
        }
        None => error!("{KEY_ART_SCENE} missing"),
    }

    let Some(menu_scene) = load_scene(&data, MENU_SCENE) else {
        error!("{MENU_SCENE} missing");
        return;
    };
    let menu = stage::spawn_scene(&mut ctx, &menu_scene, MENU_LAYER, 1, Shading::Ui, |_| false);
    state.camera = menu.camera;
    // Screen-space UI (the fade fill, the status line) draws with the menu camera, after every
    // 3D scene and the menu itself.
    if let Some(cam) = menu.camera {
        ctx.commands.entity(cam).insert(IsDefaultUiCamera);
    }
    // bgMain:transitionTo plays the menu's show chore: its black fill fades out over a second.
    if let Some(c) = load_chore(&data, MENU_SHOW_CHORE) {
        let p = ChorePlayer::new(&data, c, Arc::new(menu.agents.clone()), false);
        ctx.commands.spawn((Name::new(MENU_SHOW_CHORE), p, DespawnWhenDone));
    }

    // The list menu agent: extents from ui_menu.prop, at the scene origin, then widened by its
    // "Aspect Ratio - Chore" held at (aspect - 4/3) / (4/9), as AspectRatio.lua does.
    let menu_props = ctx.props.file(&data, ttmeta::Symbol::of(MENU_PROP));
    let base_extents =
        menu_props.as_deref().and_then(|p| extents(ctx.props, &data, p)).unwrap_or((Vec2::new(-5.75, -3.8), Vec2::new(5.75, 2.5)));
    let aspect_chore = menu_props
        .as_deref()
        .and_then(|p| ctx.props.symbol(&data, p, "Aspect Ratio - Chore"))
        .and_then(|s| data.read_symbol(s))
        .and_then(|b| ttmeta::Chore::parse(&b).ok());
    let (menu_min, menu_max) = menu_extents(base_extents, aspect_chore.as_ref(), aspect);

    // The title widget is an agent of the menu scene; it belongs to the main page only.
    let mut title_height = 0.0;
    let mut title_extents = None;
    if let Some(&title) = menu.agents.get("ui_menuMain_title") {
        if let Some(ext) = extents(ctx.props, &data, &agent_props(&menu_scene, "ui_menuMain_title")) {
            let pos = Vec3::new(menu_min.x - ext.0.x, menu_max.y - ext.1.y, 0.0);
            ctx.commands.entity(title).insert(Transform::from_translation(pos));
            title_height = ext.1.y - ext.0.y;
            title_extents = Some(ext);
        }
        state.title = Some(title);
    }

    let mut templates = HashMap::new();
    for (w, scene) in [(Widget::Button, "ui_listButton.scene"), (Widget::Toggle, "ui_listToggle.scene"), (Widget::Slider, "ui_listSlider.scene"), (Widget::Popup, "ui_popupButton.scene")] {
        match load_scene(&data, scene) {
            Some(s) => {
                templates.insert(w, s);
            }
            None => error!("{scene} missing"),
        }
    }
    let mut chores = HashMap::new();
    for (w, prefix) in [(Widget::Button, "ui_listButton"), (Widget::Toggle, "ui_listToggle"), (Widget::Slider, "ui_listSlider"), (Widget::Popup, "ui_popupButton")] {
        chores.insert(
            w,
            WidgetChores {
                select: load_chore(&data, &format!("{prefix}_select.chore")),
                deselect: load_chore(&data, &format!("{prefix}_deselect.chore")),
                press: load_chore(&data, &format!("{prefix}_press.chore")),
            },
        );
    }
    // The status button's chores (two of their file names are spelled "Staus" in the game).
    chores.insert(
        Widget::Status,
        WidgetChores {
            select: load_chore(&data, "ui_episodeSelect_episodeStausSelect.chore"),
            deselect: load_chore(&data, "ui_episodeSelect_episodeStausDeselect.chore"),
            press: load_chore(&data, "ui_episodeSelect_episodeStatusPress.chore"),
        },
    );
    chores.insert(
        Widget::Appearance,
        WidgetChores {
            select: load_chore(&data, "ui_characterSelect_appearanceButtonSelect.chore"),
            deselect: load_chore(&data, "ui_characterSelect_appearanceButtonDeselect.chore"),
            press: load_chore(&data, "ui_characterSelect_appearanceButtonPress.chore"),
        },
    );
    let legend = ctx.props.file(&data, ttmeta::Symbol::of(LEGEND_PROP));
    let episodes = ctx
        .props
        .file(&data, ttmeta::Symbol::of("ui_episode_info.prop"))
        .and_then(|p| ctx.props.int(&data, &p, "Episode Count"))
        .unwrap_or(5)
        .max(1) as u32;
    commands.insert_resource(MenuAssets {
        templates,
        header: load_scene(&data, "ui_header.scene"),
        tooltip: load_scene(&data, "ui_tooltipDisplay.scene"),
        popup: load_scene(&data, "ui_popup.scene"),
        chores,
        dlog: data.read(MENU_DIALOG).unwrap_or_default(),
        language: ttmeta::text::LanguageDb::parse(&data.read(MENU_LANGUAGE).unwrap_or_default()),
        legend,
        episodes,
        title_height,
        title_extents,
        base_extents,
        aspect_chore,
        aspect,
        menu_min,
        menu_max,
    });
    state.stack = vec![Page::Main];
    state.selections = vec![0];
    state.rebuild = true;
    // MCSM_PLAYER_CHOICE=1..6: as if character select had been completed (screenshots).
    state.player_choice = std::env::var("MCSM_PLAYER_CHOICE").ok().and_then(|v| v.parse().ok()).filter(|c| (1..=APPEARANCES).contains(c));
    // MCSM_MENU_PAGE=settings|av|gameplay|graphics|confirm: open a page directly (screenshots).
    if let Ok(p) = std::env::var("MCSM_MENU_PAGE") {
        let pages: &[Page] = match p.as_str() {
            "settings" => &[Page::Settings],
            "av" => &[Page::Settings, Page::AudioVideo],
            "gameplay" => &[Page::Settings, Page::Gameplay],
            "graphics" => &[Page::Settings, Page::Graphics],
            "confirm" => &[Page::Settings, Page::ConfirmSettings],
            "characters" => &[Page::CharacterSelect],
            "episodes" => {
                state.episode = 1;
                state.show_pending = true;
                &[Page::EpisodeSelect]
            }
            _ => &[],
        };
        for &page in pages {
            state.push(page);
        }
    }

    // Background transitions fade this black fill over everything (the game's fillForeground
    // agents, one per background).
    commands.spawn((
        FadeOverlay,
        Node { position_type: PositionType::Absolute, left: px(0), top: px(0), width: percent(100), height: percent(100), ..default() },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
        GlobalZIndex(i32::MAX - 1),
        bevy::ui::FocusPolicy::Pass,
    ));
    commands.spawn((
        StatusLine,
        Text::new(""),
        TextFont { font_size: bevy::text::FontSize::Px(18.0), ..default() },
        Node { position_type: PositionType::Absolute, bottom: px(12), left: px(16), ..default() },
    ));
}

/// What a page holds, in `Populate` order.
enum Entry {
    Header(&'static str),
    Item { name: &'static str, label: &'static str, kind: KindSpec, tooltip: Option<&'static str> },
    Tooltip,
}

enum KindSpec {
    Button(Action),
    /// (key, values, display labels).
    Toggle(&'static str, Vec<SettingValue>, Vec<&'static str>),
    Slider(&'static str),
    Resolution,
}

fn page_entries(page: Page) -> (Vec<Entry>, bool) {
    use KindSpec::*;
    use SettingValue::{Bool, Float};
    let item = |name, label, kind, tooltip| Entry::Item { name, label, kind, tooltip };
    let on_off = || vec!["label_on", "label_off"];
    match page {
        Page::Main => (
            vec![
                item("play", "label_play", Button(Action::Open(Page::EpisodeSelect)), None),
                item("episodes", "label_store", Button(Action::Episodes), None),
                item("savesFiles", "label_saveFiles", Button(Action::SaveFiles), None),
                item("stats", "label_stats", Button(Action::Stats), None),
                item("settings", "label_settings", Button(Action::Open(Page::Settings)), None),
                item("exit", "label_exitGame", Button(Action::Exit), None),
            ],
            false,
        ),
        Page::Settings => (
            vec![
                Entry::Header("header_settings"),
                item("controls", "label_controls", Button(Action::Controls), None),
                item("av", "label_audiovideo", Button(Action::Open(Page::AudioVideo)), None),
                item("gameplay", "label_gameplay", Button(Action::Open(Page::Gameplay)), None),
                item("graphics", "label_graphics", Button(Action::Open(Page::Graphics)), None),
                item("credits", "label_credits", Button(Action::Credits), None),
            ],
            true,
        ),
        Page::AudioVideo => (
            vec![
                Entry::Header("header_audiovideo"),
                item("master", "label_volume_master", Slider(settings::VOLUME_MASTER), Some("tooltip_volumeMaster")),
                item("music", "label_volume_music", Slider(settings::VOLUME_MUSIC), Some("tooltip_volumeMusic")),
                item("subtitles", "label_subtitles", Toggle(settings::SUBTITLES, vec![Bool(true), Bool(false)], on_off()), Some("tooltip_subtitles")),
                item(
                    "subtitleSize",
                    "label_subtitleSize",
                    Toggle(settings::SUBTITLE_SIZE, vec![Float(0.0), Float(0.5), Float(1.0)], vec!["label_small", "label_medium", "label_large"]),
                    Some("tooltip_subtitleSize"),
                ),
                item("brightness", "label_brightness", Slider(settings::BRIGHTNESS), Some("tooltip_brightness")),
                Entry::Tooltip,
            ],
            true,
        ),
        Page::Gameplay => (
            vec![
                Entry::Header("header_gameplay"),
                item("invertY", "label_inverty", Toggle(settings::INVERT_Y, vec![Bool(true), Bool(false)], on_off()), Some("tooltip_invertY")),
                item(
                    "showDots",
                    "label_displayStyle",
                    Toggle(settings::RETICLE_DOTS, vec![Bool(true), Bool(false)], vec!["label_standard", "label_minimal"]),
                    Some("tooltip_displayStyle"),
                ),
                Entry::Tooltip,
            ],
            true,
        ),
        Page::Graphics => (
            vec![
                Entry::Header("header_graphics"),
                item("tq", "label_texturequality", Toggle(settings::TEXTURE_QUALITY, vec![Float(1.0), Float(0.0)], vec!["label_low", "label_high"]), Some("tooltip_textureQuality")),
                item("aa", "label_antialiasing", Toggle(settings::ANTI_ALIASING, vec![Bool(true), Bool(false)], on_off()), Some("tooltip_antialiasing")),
                // "Windowed", "inverted": shown as Full Screen On/Off.
                item("windowed", "label_windowed", Toggle(settings::WINDOWED, vec![Bool(false), Bool(true)], on_off()), Some("tooltip_fullScreen")),
                item("resolution", "label_resolution", Resolution, Some("tooltip_resolution")),
                Entry::Tooltip,
            ],
            true,
        ),
        Page::ConfirmSettings => (Vec::new(), false),
        Page::EpisodeSelect => (Vec::new(), true),
        Page::CharacterSelect => (Vec::new(), true),
    }
}

fn spawn_widget(ctx: &mut Ctx, template: &Scene, reference: &str, layer: usize, parent: Entity) -> Option<(Entity, Arc<HashMap<String, Entity>>)> {
    let skip = |n: &str| n.starts_with("cam_") || n.ends_with("_new") || n == "new" || n == "lock" || n == "splatter" || n == "select";
    let spawned = stage::spawn_scene(ctx, template, layer, 1, Shading::Ui, skip);
    let root = *spawned.agents.get(reference)?;
    // Widget:Show unhides the widget (some templates start hidden, e.g. the header).
    ctx.commands.entity(root).insert((ChildOf(parent), Visibility::Inherited));
    // Clone() roots the whole template at the reference: agents attached to nothing go under it.
    for a in &template.agents {
        if a.name == reference || a.name == template.name || skip(&a.name) {
            continue;
        }
        let attached = match ctx.props.get(ctx.data, &a.props, "Scene: Location Info") {
            Some(ttmeta::Value::Location(l)) => !l.attached_agent.is_empty(),
            _ => false,
        };
        if let (false, Some(&e)) = (attached, spawned.agents.get(&a.name)) {
            ctx.commands.entity(e).insert(ChildOf(root));
        }
    }
    Some((root, Arc::new(spawned.agents)))
}

/// Spawn `text` on a template agent, styled by that agent's props; records the main-pass
/// materials so chores can recolor it.
fn spawn_label(ctx: &mut Ctx, template: &Scene, agents: &HashMap<String, Entity>, agent: &str, text: &str, layers: &RenderLayers) -> Option<TextSlot> {
    let &entity = agents.get(agent)?;
    let style = TextStyle::from_props(ctx.props, ctx.data, &agent_props(template, agent))?;
    let anchor = ctx.commands.spawn((Transform::IDENTITY, Visibility::Inherited, layers.clone(), ChildOf(entity))).id();
    let (_, mats) = text::spawn_text(ctx, anchor, text, &style, layers);
    ctx.commands.entity(entity).entry::<AgentMaterials>().or_default().and_modify(move |mut m| m.text = mats);
    Some(TextSlot { agent: entity, anchor, style })
}

fn text_width(ctx: &mut Ctx, text: &str, style: &TextStyle) -> f32 {
    let Some(font) = ctx.cache.font(ctx.data, ctx.images, style.font) else { return 0.0 };
    text.chars().filter_map(|c| font.0.glyphs.get(&(c as u32))).map(|g| g.advance).sum::<f32>() * style.unit
}

fn value_text(kind: &Kind, settings: &Settings) -> Option<String> {
    match kind {
        Kind::Button(_) => None,
        Kind::Toggle { key, label, values, display } => {
            let v = settings.get(key);
            let i = values.iter().position(|x| *x == v).unwrap_or(0);
            Some(format!("{label} : {}", display.get(i).cloned().unwrap_or_default()))
        }
        Kind::Slider { key, label, resolutions, .. } => Some(match (resolutions, settings.get(key)) {
            (Some(_), SettingValue::Size(w, h)) => format!("{label} : {w}x{h}"),
            (_, v) => format!("{label} : {}", (slider_value(v) * 10.0 + 0.5).floor() as i32),
        }),
    }
}

fn slider_value(v: SettingValue) -> f32 {
    match v {
        SettingValue::Float(f) => f,
        SettingValue::Bool(b) => b as u8 as f32,
        SettingValue::Size(..) => 0.0,
    }
}

/// A slider's position as 0..1.
fn slider_portion(kind: &Kind, settings: &Settings) -> f32 {
    match kind {
        Kind::Slider { key, resolutions: Some(r), .. } => {
            let SettingValue::Size(w, h) = settings.get(key) else { return 0.0 };
            let i = r.iter().position(|&s| s == (w, h)).unwrap_or(0);
            if r.len() > 1 { i as f32 / (r.len() - 1) as f32 } else { 0.0 }
        }
        Kind::Slider { key, .. } => slider_value(settings.get(key)).clamp(0.0, 1.0),
        _ => 0.0,
    }
}

/// The character-select 3D scene (Male / Female Jesse): its agents and camera.
#[derive(Resource)]
pub struct CharacterSelect {
    agents: Arc<HashMap<String, Entity>>,
    camera: Option<Entity>,
}

/// `Menu_CharacterSelect`: the header, the six appearance buttons (female 1-3, male 4-6; their
/// thumbnails come from each button's "<texture> - Texture"), over the character scene.
fn build_character_select(ctx: &mut Ctx, menu: &MenuAssets, state: &mut MenuState, root: Entity) {
    let data = ctx.data;
    let Some(scene) = load_scene(data, CHARACTER_SELECT_SCENE) else {
        error!("{CHARACTER_SELECT_SCENE} missing");
        return;
    };
    let layers = RenderLayers::layer(MENU_LAYER);
    let skip = |n: &str| n.starts_with("cam_") || n == "ui_characterSelect_fillForeground";
    let spawned = stage::spawn_scene(ctx, &scene, MENU_LAYER, 1, Shading::Ui, skip);
    state.roots.extend(spawned.agents.values().copied());
    if let Some(&owner) = spawned.agents.get("ui_characterSelect") {
        ctx.commands.entity(owner).insert(ChildOf(root));
    }
    if let Some(text) = menu.text("header_characterSelect") {
        spawn_label(ctx, &scene, &spawned.agents, "ui_characterSelect_header", &text, &layers);
    }
    let mut count = 0;
    for i in 1..=APPEARANCES {
        let name = format!("ui_characterSelect_appearanceButton{i}");
        let Some(&button) = spawned.agents.get(&name) else { continue };
        let ext = extents(ctx.props, data, &agent_props(&scene, &name)).unwrap_or((Vec2::splat(-0.8), Vec2::splat(0.8)));
        // The button chores animate "default": the button they are played on.
        let mut agents = HashMap::new();
        agents.insert("default".to_owned(), button);
        ctx.commands.entity(button).insert(MenuItem {
            index: (i - 1) as usize,
            kind: Kind::Button(Action::ChooseAppearance(i)),
            widget: Widget::Appearance,
            extents: ext,
            agents: Arc::new(agents),
            text: None,
            tooltip: None,
            handle: None,
        });
        count += 1;
    }
    state.count = count;
    state.shown = None;
}

/// `ui_characterSelect_keyArt.scene`'s texture property per look (index 1-3 within a gender).
const MALE_TEXTURES: [&str; 3] = ["", "skM1_jesseMaleA.d3dtx", "skM1_jesseMaleB.d3dtx"];
const FEMALE_TEXTURES: [&str; 3] = ["", "skM1_jesseFemaleA.d3dtx", "skM1_jesseFemaleB.d3dtx"];

/// Swap `<stem>.d3dtx` for `replacement` (or back to the original with `None`) on an agent's
/// mesh materials.
fn swap_texture(
    mats: &AgentMaterials,
    stem: &str,
    replacement: Option<ttmeta::Symbol>,
    data: &GameData,
    cache: &mut AssetCache,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    lit_materials: &mut Assets<crate::lit::LitMaterial>,
) {
    for (h, _, original) in &mats.lit {
        let Some(original) = original else { continue };
        let Some(name) = data.name_of(*original) else { continue };
        if !name.strip_suffix(".d3dtx").unwrap_or(name).eq_ignore_ascii_case(stem) {
            continue;
        }
        let tex = cache.image(data, images, replacement.unwrap_or(*original), true);
        if let Some(mut m) = lit_materials.get_mut(h) {
            m.base.base_color_texture = tex;
        }
    }
    for (h, original) in &mats.diffuse {
        let Some(name) = data.name_of(*original) else { continue };
        if !name.strip_suffix(".d3dtx").unwrap_or(name).eq_ignore_ascii_case(stem) {
            continue;
        }
        let tex = cache.image(data, images, replacement.unwrap_or(*original), true);
        if let Some(mut m) = materials.get_mut(h) {
            m.base_color_texture = tex;
        }
    }
}

/// `Menu_CharacterSelect_AppearanceSelect` (the selected button's look: its performance chore,
/// texture and gender) and the episode menu's `appearanceSwap` (Jesse in the diorama).
#[allow(clippy::too_many_arguments)]
pub fn appearance(
    mut commands: Commands,
    data: Option<Res<GameData>>,
    character_select: Option<Res<CharacterSelect>>,
    diorama: Option<Res<Diorama>>,
    mut state: ResMut<MenuState>,
    agent_materials: Query<&AgentMaterials>,
    mut cache: ResMut<AssetCache>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut lit_materials: ResMut<Assets<crate::lit::LitMaterial>>,
) {
    let Some(data) = data else { return };
    if let (Some(cs), Page::CharacterSelect) = (character_select.as_ref(), state.top()) {
        let choice = state.selected() as u8 + 1;
        if state.appearance_shown != Some(choice) {
            let (Some(&male), Some(&female)) = (cs.agents.get("Male"), cs.agents.get("Female")) else { return };
            let (Ok(male_mats), Ok(female_mats)) = (agent_materials.get(male), agent_materials.get(female)) else { return };
            let is_female = choice <= 3;
            let index = ((choice - 1) % 3) as usize;
            let (mats, stem, table) =
                if is_female { (female_mats, "skM1_jesseFemale", FEMALE_TEXTURES) } else { (male_mats, "skM1_jesseMale", MALE_TEXTURES) };
            let replacement = Some(table[index]).filter(|t| !t.is_empty()).map(ttmeta::Symbol::of);
            swap_texture(mats, stem, replacement, &data, &mut cache, &mut images, &mut materials, &mut lit_materials);
            commands.entity(female).insert(if is_female { Visibility::Inherited } else { Visibility::Hidden });
            commands.entity(male).insert(if is_female { Visibility::Hidden } else { Visibility::Inherited });
            if let Some(old) = state.appearance_chore.take() {
                commands.entity(old).try_despawn();
            }
            if let Some(c) = load_chore(&data, &format!("ui_characterSelect_appearance{choice}.chore")) {
                let player = commands.spawn((ChorePlayer::new(&data, c, cs.agents.clone(), false), DespawnWhenDone)).id();
                state.appearance_chore = Some(player);
            }
            state.appearance_shown = Some(choice);
        }
    }
    // appearanceSwap: kMaleTextures [5] / [6], kFemaleTextures [2] / [3] on the diorama's Jesse.
    if let (Some(d), Some(choice)) = (diorama.as_ref(), state.player_choice) {
        if state.diorama_appearance != Some(choice) {
            let replacement = match choice {
                2 | 5 => Some("skM1_jesseA.d3dtx"),
                3 | 6 => Some("skM1_jesseB.d3dtx"),
                _ => None,
            };
            for actor in ["Jesse_episode1", "Jesse_episode3", "Jesse_episode4"] {
                let Some(mats) = d.agents.get(actor).and_then(|&e| agent_materials.get(e).ok()) else { return };
                swap_texture(mats, "skM1_jesse", replacement.map(ttmeta::Symbol::of), &data, &mut cache, &mut images, &mut materials, &mut lit_materials);
            }
            state.diorama_appearance = Some(choice);
        }
    }
}

/// Build the page on top of the stack (and, for a popup, the page under it).
#[allow(clippy::too_many_arguments)]
pub fn rebuild(
    mut commands: Commands,
    data: Option<Res<GameData>>,
    menu: Option<Res<MenuAssets>>,
    settings: Option<Res<Settings>>,
    mut assets: SceneAssets,
    mut state: ResMut<MenuState>,
    monitors: Query<&bevy::window::Monitor>,
    diorama: Option<Res<Diorama>>,
    character_select: Option<Res<CharacterSelect>>,
) {
    let (Some(data), Some(menu), Some(settings)) = (data, menu, settings) else { return };
    if !state.rebuild {
        return;
    }

    state.rebuild = false;
    for root in std::mem::take(&mut state.roots) {
        commands.entity(root).try_despawn();
    }
    state.popup_camera = None;
    let top = state.top();
    let page = if top == Page::ConfirmSettings { Page::Settings } else { top };
    if let Some(title) = state.title {
        commands.entity(title).insert(if page == Page::Main { Visibility::Inherited } else { Visibility::Hidden });
    }
    let data = data.clone();
    let mut ctx = assets.ctx(&mut commands, &data);
    let layers = RenderLayers::layer(MENU_LAYER);
    let interactive = top != Page::ConfirmSettings;
    let root = ctx.commands.spawn((Name::new(format!("menu page {page:?}")), Transform::IDENTITY, Visibility::Inherited, layers.clone())).id();
    state.roots.push(root);

    let (min, max) = (menu.menu_min, menu.menu_max);
    let mut cursor_y = max.y;
    // On the main page the title is the list's first widget.
    if page == Page::Main && state.title.is_some() {
        cursor_y -= menu.title_height;
    }
    let (entries, has_back) = page_entries(page);
    let mut index = 0;
    let mut tooltip_wanted = false;
    for entry in entries {
        match entry {
            Entry::Header(label) => {
                let Some(scene) = &menu.header else { continue };
                let Some((h, agents)) = spawn_widget(&mut ctx, scene, "ui_header_reference", MENU_LAYER, root) else { continue };
                let ext = extents(ctx.props, &data, &agent_props(scene, "ui_header_reference")).unwrap_or((Vec2::new(-5.75, -0.25), Vec2::new(5.75, 0.25)));
                // Header:Place: just above the list's top edge; its extents shrink to the text, and
                // the text's left edge meets the menu's (the text itself is centered on its agent).
                let text = menu.text(label).unwrap_or_default();
                let slot = spawn_label(&mut ctx, scene, &agents, "ui_header_header", &text, &layers);
                let width = slot.as_ref().map_or(0.0, |s| text_width(&mut ctx, &text, &s.style));
                let left = match slot.as_ref().map(|s| s.style.align) {
                    Some(2) => -width * 0.5,
                    Some(3) => -width,
                    _ => 0.0,
                };
                ctx.commands.entity(h).insert(Transform::from_xyz(min.x - left, max.y - ext.0.y, 0.0));
            }
            Entry::Tooltip => tooltip_wanted = true,
            Entry::Item { name, label, kind, tooltip } => {
                let Some(text) = menu.text(label) else {
                    warn!("menu label {label:?} has no resolvable text yet; skipping the {name} widget");
                    continue;
                };
                let (widget, kind) = match kind {
                    KindSpec::Button(a) => (Widget::Button, Kind::Button(a)),
                    KindSpec::Toggle(key, values, display) => (
                        Widget::Toggle,
                        Kind::Toggle { key, label: text.clone(), values, display: display.iter().map(|d| menu.text(d).unwrap_or_else(|| d.to_string())).collect() },
                    ),
                    KindSpec::Slider(key) => (Widget::Slider, Kind::Slider { key, label: text.clone(), step: 0.1, resolutions: None }),
                    KindSpec::Resolution => {
                        let modes: Vec<(u32, u32)> = monitors.iter().flat_map(|m| m.video_modes.iter().map(|v| (v.physical_size.x, v.physical_size.y))).collect();
                        let current = match settings.get(settings::WINDOW_SIZE) {
                            SettingValue::Size(w, h) => (w, h),
                            _ => (1280, 720),
                        };
                        (Widget::Slider, Kind::Slider { key: settings::WINDOW_SIZE, label: text.clone(), step: 1.0, resolutions: Some(settings::resolutions(&modes, current)) })
                    }
                };
                let Some(template) = menu.templates.get(&widget) else { continue };
                let (reference, label_agent) = match widget {
                    Widget::Button => ("ui_listButton_reference", "ui_listButton_label"),
                    Widget::Toggle => ("ui_listToggle_reference", "ui_listToggle_label"),
                    Widget::Slider => ("ui_listSlider_reference", "ui_listSlider_label"),
                    Widget::Popup => ("ui_popupButton_reference", "ui_popupButton_label"),
                    Widget::Status | Widget::Arrow | Widget::Appearance => continue,
                };
                let Some((w_root, agents)) = spawn_widget(&mut ctx, template, reference, MENU_LAYER, root) else { continue };
                let ext = extents(ctx.props, &data, &agent_props(template, reference)).unwrap_or((Vec2::new(-2.5, -0.4), Vec2::new(2.5, 0.4)));
                let pos = Vec3::new(min.x - ext.0.x, cursor_y - ext.1.y, 0.0);
                cursor_y = pos.y + ext.0.y;
                ctx.commands.entity(w_root).insert((Name::new(format!("menu_{name}")), Transform::from_translation(pos)));
                let shown = value_text(&kind, &settings).unwrap_or_else(|| text.clone());
                let slot = spawn_label(&mut ctx, template, &agents, label_agent, &shown, &layers);
                let handle = if widget == Widget::Slider {
                    let bar = extents(ctx.props, &data, &agent_props(template, "ui_listSlider_background"));
                    let h = extents(ctx.props, &data, &agent_props(template, "ui_listSlider_handle"));
                    let base = agent_props(template, "ui_listSlider_handle");
                    let yz = match ctx.props.get(&data, &base, "Scene: Location Info") {
                        Some(ttmeta::Value::Location(l)) => Vec2::new(l.local.pos[1], 0.0),
                        _ => Vec2::new(0.0, 0.01),
                    };
                    match (agents.get("ui_listSlider_handle"), bar, h) {
                        (Some(&e), Some((bmin, bmax)), Some((hmin, hmax))) => Some((e, bmin.x - hmin.x, bmax.x - hmax.x, yz)),
                        _ => None,
                    }
                } else {
                    None
                };
                if let Some((e, l, r, yz)) = handle {
                    let x = l + (r - l) * slider_portion(&kind, &settings);
                    ctx.commands.entity(e).insert(Transform::from_xyz(x, yz.x, yz.y));
                }
                let tooltip = tooltip.and_then(|t| menu.text(t));
                ctx.commands.entity(w_root).insert(MenuItem { index, kind, widget, extents: ext, agents, text: slot, tooltip, handle });
                index += 1;
            }
        }
    }
    if interactive {
        state.count = index;
        let sel = state.selected().min(index.saturating_sub(1));
        state.select(sel);
        state.shown = None;
    }

    // TooltipDisplay: bottom-left of the menu, wrapping at 8 units, growing upward.
    if tooltip_wanted {
        if let Some(scene) = &menu.tooltip {
            if let Some(mut style) = TextStyle::from_props(ctx.props, &data, &agent_props(scene, "ui_tooltipDisplay_label")) {
                // tooltipDisplay.alignmentH = menu.align ("left").
                style.align = 1;
                let anchor = ctx.commands.spawn((Transform::from_xyz(min.x, min.y, 0.0), Visibility::Inherited, layers.clone(), ChildOf(root))).id();
                ctx.commands.entity(root).insert(TooltipLine { anchor, style, shown: None });
            }
        }
    }

    // Legend: "Select", and "Previous Menu" on pages that can go back (clickable with the mouse).
    if let (Some(legend_props), Some(select)) = (menu.legend.clone(), menu.text("legend_select")) {
        if let Some(mut style) = TextStyle::from_props(ctx.props, &data, &legend_props) {
            style.align = 1;
            let image_scale = ctx.props.float(&data, &legend_props, "Text Image Scale").unwrap_or(1.0);
            let legend = ctx.commands.spawn((Name::new("menu legend"), Transform::from_xyz(min.x, LEGEND_Y, 0.0), Visibility::Inherited, layers.clone(), ChildOf(root))).id();
            let mut x = 0.0;
            let mut parts = Vec::new();
            if page == Page::CharacterSelect {
                // Legend_Add("analogStickLeft", "legend_changeAppearance"), select, back.
                parts.push(("ui_legend_analogStickLeft.d3dtx", menu.text("legend_changeAppearance").unwrap_or_default()));
            }
            parts.push(("ui_legend_faceButtonDown.d3dtx", select));
            if page == Page::CharacterSelect {
                parts.push(("ui_legend_faceButtonRight.d3dtx", menu.text("legend_back").unwrap_or_default()));
            } else if has_back {
                parts.push(("ui_legend_faceButtonRight.d3dtx", menu.text("legend_previousMenu").unwrap_or_default()));
            }
            let back_part = parts.len() - 1;
            for (i, (icon, label)) in parts.into_iter().enumerate() {
                let start = x;
                if let Some(icon) = ctx.cache.image(&data, ctx.images, ttmeta::Symbol::of(icon), false) {
                    let px = ctx.images.get(&icon).map_or(46.0, |i| i.width() as f32);
                    let size = px * image_scale * style.unit;
                    let quad = ctx.meshes.add(Rectangle::new(size, size));
                    let material = ctx.materials.add(StandardMaterial { base_color_texture: Some(icon), unlit: true, alpha_mode: AlphaMode::Blend, cull_mode: None, ..default() });
                    ctx.commands.spawn((Mesh3d(quad), MeshMaterial3d(material), Transform::from_xyz(x + size * 0.5, 0.0, 0.01), layers.clone(), ChildOf(legend)));
                    x += size;
                }
                let anchor = ctx.commands.spawn((Transform::from_xyz(x, 0.0, 0.0), Visibility::Inherited, layers.clone(), ChildOf(legend))).id();
                let (w, _) = text::spawn_text(&mut ctx, anchor, &format!(" {label}   "), &style, &layers);
                x += w;
                if has_back && i == back_part {
                    ctx.commands.entity(legend).insert(LegendBack(Vec2::new(min.x + start, LEGEND_Y - 0.3), Vec2::new(min.x + x, LEGEND_Y + 0.3)));
                }
            }
        }
    }

    if top == Page::ConfirmSettings {
        build_popup(&mut ctx, &menu, &mut state);
    }
    if page == Page::CharacterSelect {
        if character_select.is_none() {
            if let Some(cs) = spawn_character_art(&mut ctx) {
                ctx.commands.insert_resource(cs);
            }
        }
        build_character_select(&mut ctx, &menu, &mut state, root);
    }
    if page == Page::EpisodeSelect {
        let agents = match &diorama {
            Some(d) => d.agents.clone(),
            None => match spawn_diorama(&mut ctx, &menu) {
                Some(d) => {
                    let agents = d.agents.clone();
                    ctx.commands.insert_resource(d);
                    agents
                }
                None => Arc::new(HashMap::new()),
            },
        };
        if state.show_pending {
            state.show_pending = false;
            let n = state.episode;
            for chore in [format!("ui_diorama_episode{n}Setup.chore"), format!("ui_diorama_episode{n}Show.chore")] {
                play_diorama(ctx.commands, &data, &agents, &chore);
            }
            // The camera and lights rest where a slide into this episode ends: hold the end of
            // the transition from a neighbour.
            let from = if n > 1 { n - 1 } else { n + 1 };
            if let Some(c) = load_chore(&data, &format!("ui_diorama_transition{from}to{n}.chore")) {
                let mut p = ChorePlayer::new(&data, c, agents.clone(), false);
                p.time = p.chore.length;
                ctx.commands.spawn((p, DespawnWhenDone));
            }
        }
        build_episode(&mut ctx, &menu, &mut state, root);
    }
}

/// `ui_characterSelect_keyArt.scene`: the 3D Jesse scene behind character select.
fn spawn_character_art(ctx: &mut Ctx) -> Option<CharacterSelect> {
    let Some(scene) = load_scene(ctx.data, CHARACTER_SELECT_ART) else {
        error!("{CHARACTER_SELECT_ART} missing");
        return None;
    };
    // The panic-meter squares are attached to an agent this scene lacks.
    let spawned = stage::spawn_scene(ctx, &scene, CHARACTER_LAYER, 0, Shading::World, |n| n.starts_with("ui_panicMeter_"));
    Some(CharacterSelect { agents: Arc::new(spawned.agents), camera: spawned.camera })
}

/// `ui_diorama.scene`, every episode's set resting in its "Hold" pose (the show chore held at
/// its end, so sets the camera passes are never in T-pose), with all of its chores loaded.
fn spawn_diorama(ctx: &mut Ctx, menu: &MenuAssets) -> Option<Diorama> {
    let data = ctx.data;
    let Some(scene) = load_scene(data, DIORAMA_SCENE) else {
        error!("{DIORAMA_SCENE} missing");
        return None;
    };
    let spawned = stage::spawn_scene_with_camera(ctx, &scene, DIORAMA_LAYER, 0, Shading::World, |_| false, Some("cam_ui_diorama"));
    let agents = Arc::new(spawned.agents);
    for n in 1..=menu.episodes {
        if let Some(c) = load_chore(data, &format!("ui_diorama_episode{n}Hold.chore")) {
            let mut p = ChorePlayer::new(data, c, agents.clone(), false);
            p.time = p.chore.length;
            ctx.commands.spawn((p, DespawnWhenDone));
        }
        let _ = load_chore(data, &format!("ui_diorama_episode{n}Setup.chore"));
        let _ = load_chore(data, &format!("ui_diorama_episode{n}Show.chore"));
        let _ = load_chore(data, &format!("ui_diorama_transition{n}to{}.chore", n + 1));
        let _ = load_chore(data, &format!("ui_diorama_transition{}to{n}.chore", n + 1));
    }
    Some(Diorama { agents, camera: spawned.camera })
}

/// Build the character-select and diorama scenes ahead of time, a few seconds into the main
/// menu (cameras off), so Play opens them at once instead of loading then.
pub fn prewarm(
    mut commands: Commands,
    time: Res<Time>,
    data: Option<Res<GameData>>,
    menu: Option<Res<MenuAssets>>,
    state: Res<MenuState>,
    diorama: Option<Res<Diorama>>,
    character_select: Option<Res<CharacterSelect>>,
    mut assets: SceneAssets,
) {
    let (Some(data), Some(menu)) = (data, menu) else { return };
    if state.transition.is_some() || state.top() != Page::Main {
        return;
    }
    let t = time.elapsed_secs();
    let data = data.clone();
    if character_select.is_none() && t > 4.0 {
        let mut ctx = assets.ctx(&mut commands, &data);
        if let Some(cs) = spawn_character_art(&mut ctx) {
            if let Some(cam) = cs.camera {
                ctx.commands.entity(cam).entry::<Camera>().and_modify(|mut c| c.is_active = false);
            }
            ctx.commands.insert_resource(cs);
        }
    } else if character_select.is_some() && diorama.is_none() && t > 6.0 {
        let mut ctx = assets.ctx(&mut commands, &data);
        if let Some(d) = spawn_diorama(&mut ctx, &menu) {
            if let Some(cam) = d.camera {
                ctx.commands.entity(cam).entry::<Camera>().and_modify(|mut c| c.is_active = false);
            }
            ctx.commands.insert_resource(d);
        }
    }
}

/// bgMain / bgCharacterSelect / bgEpisodeSelect: only the current page's 3D scene is drawn.
pub fn sync_cameras(
    state: Res<MenuState>,
    diorama: Option<Res<Diorama>>,
    character_select: Option<Res<CharacterSelect>>,
    mut cameras: Query<&mut Camera>,
) {
    let on_episodes = state.top() == Page::EpisodeSelect;
    let on_characters = state.top() == Page::CharacterSelect;
    for (cam, active) in [
        (state.key_art_camera, !on_episodes && !on_characters),
        (diorama.as_ref().and_then(|d| d.camera), on_episodes),
        (character_select.as_ref().and_then(|c| c.camera), on_characters),
    ] {
        if let Some(mut c) = cam.and_then(|e| cameras.get_mut(e).ok()) {
            if c.is_active != active {
                c.is_active = active;
            }
        }
    }
}

/// Play a chore on the diorama's agents; returns its length.
fn play_diorama(commands: &mut Commands, data: &GameData, agents: &Arc<HashMap<String, Entity>>, name: &str) -> f32 {
    let Some(c) = load_chore(data, name) else { return 0.0 };
    let length = c.length;
    commands.spawn((Name::new(name.to_owned()), ChorePlayer::new(data, c, agents.clone(), false), DespawnWhenDone));
    length
}

/// `Menu_EpisodeSelect` / `Menu_EpisodeSelect_SlideComplete`: the episode's title and status
/// button, and the arrows (hidden at either end, and everything hidden while sliding).
fn build_episode(ctx: &mut Ctx, menu: &MenuAssets, state: &mut MenuState, root: Entity) {
    let data = ctx.data;
    let Some(scene) = load_scene(data, EPISODE_SCENE) else {
        error!("{EPISODE_SCENE} missing");
        return;
    };
    let layers = RenderLayers::layer(MENU_LAYER);
    let skip = |n: &str| n.starts_with("cam_") || n == "ui_episodeSelect_fillForeground" || n == "ui_episodeSelect_legend" || n == "ui_episodeSelect_new";
    let spawned = stage::spawn_scene(ctx, &scene, MENU_LAYER, 1, Shading::Ui, skip);
    state.roots.extend(spawned.agents.values().copied());
    let Some(&owner) = spawned.agents.get("ui_episodeSelect") else { return };
    ctx.commands.entity(owner).insert(ChildOf(root));
    let n = state.episode;
    let sliding = state.slide.is_some();
    let shown = if sliding { Visibility::Hidden } else { Visibility::Inherited };

    // Title: Menu_Text("epi_title" .. episode).
    if let Some(&title) = spawned.agents.get("ui_episodeSelect_episodeTitle") {
        ctx.commands.entity(title).insert(shown);
        if let Some(text) = menu.text(&format!("epi_title{n}")) {
            spawn_label(ctx, &scene, &spawned.agents, "ui_episodeSelect_episodeTitle", &text, &layers);
        }
    }
    // Status: "Start Episode N" (every episode is offered as a new start here; saves and
    // installed episodes are not checked yet).
    let agents = Arc::new(spawned.agents.clone());
    if let Some(&status) = spawned.agents.get("ui_episodeSelect_episodeStatus") {
        ctx.commands.entity(status).insert(shown);
        let text = format!("{} {} {n}", menu.text("epi_start").unwrap_or_default(), menu.text("label_episode").unwrap_or_default());
        let slot = spawn_label(ctx, &scene, &spawned.agents, "ui_episodeSelect_episodeStatus", &text, &layers);
        let ext = extents(ctx.props, data, &agent_props(&scene, "ui_episodeSelect_episodeStatus")).unwrap_or((Vec2::new(-3.5, -0.3), Vec2::new(3.5, 0.3)));
        ctx.commands.entity(status).insert(MenuItem {
            index: 0,
            kind: Kind::Button(Action::StartEpisode),
            widget: Widget::Status,
            extents: ext,
            agents: agents.clone(),
            text: slot,
            tooltip: None,
            handle: None,
        });
    }

    // Arrows: ui_arrowLeft/Right.prop agents placed outside the menu's extents (MenuArrow:Place),
    // bobbing with their idle chore; the aspect chore then adjusts them for the screen.
    let menu_ext = {
        let base = ctx
            .props
            .file(data, ttmeta::Symbol::of("ui_menu_main.prop"))
            .and_then(|p| extents(ctx.props, data, &p))
            .unwrap_or((Vec2::new(-5.75, -3.8), Vec2::new(5.75, 3.8)));
        let aspect_chore = load_chore(data, EPISODE_ASPECT_CHORE);
        menu_extents(base, aspect_chore.as_deref(), menu.aspect)
    };
    let mut arrow_agents: HashMap<String, Entity> = HashMap::new();
    arrow_agents.insert("ui_episodeSelect".into(), owner);
    let arrows = [
        ("ui_episodeSelect_arrowLeft", "ui_arrowLeft.prop", Action::SlideLeft, n <= 1),
        ("ui_episodeSelect_arrowRight", "ui_arrowRight.prop", Action::SlideRight, n >= menu.episodes),
    ];
    for (index, (name, prop, action, hidden)) in arrows.into_iter().enumerate() {
        let Some(props) = ctx.props.file(data, ttmeta::Symbol::of(prop)) else { continue };
        let (amin, amax) = extents(ctx.props, data, &props).unwrap_or((Vec2::splat(-0.48), Vec2::splat(0.48)));
        let pos = if action == Action::SlideLeft {
            Vec3::new(menu_ext.0.x + amin.x, -amin.y, 0.0)
        } else {
            Vec3::new(menu_ext.1.x - amin.x, -amin.y, 0.0)
        };
        let tf = Transform::from_translation(pos);
        let vis = if hidden || sliding { Visibility::Hidden } else { Visibility::Inherited };
        let e = ctx.commands.spawn((Name::new(name), tf, vis, layers.clone(), crate::chore::BaseTransform(tf), ChildOf(owner))).id();
        let mut owned = AgentMaterials::default();
        if let Some(mesh) = ctx.props.symbol(data, &props, "D3D Mesh") {
            stage::spawn_mesh(ctx, e, &props, mesh, &layers, Shading::Ui, &[], Transform::IDENTITY, None, &mut owned);
        }
        ctx.commands.entity(e).insert(owned);
        arrow_agents.insert(name.into(), e);
        // The arrow's idle: a looping chore, whatever agent name it uses.
        if let Some(ttmeta::Value::AnimOrChore(_, chore)) = ctx.props.get(data, &props, "Walk Animation - Idle") {
            if let Some(c) = data.read_symbol(chore).and_then(|b| ttmeta::Chore::parse(&b).ok()) {
                let map: HashMap<String, Entity> = c.agents.iter().map(|a| (a.name.clone(), e)).collect();
                let player = ctx.commands.spawn(ChorePlayer::new(data, c, Arc::new(map), true)).id();
                state.roots.push(player);
            }
        }
        ctx.commands.entity(e).insert(MenuItem {
            index: index + 1,
            kind: Kind::Button(action),
            widget: Widget::Arrow,
            extents: (amin, amax),
            agents: Arc::new(HashMap::new()),
            text: None,
            tooltip: None,
            handle: None,
        });
    }
    if let Some(c) = load_chore(data, EPISODE_ASPECT_CHORE) {
        let mut p = ChorePlayer::new(data, c, Arc::new(arrow_agents), false);
        p.time = aspect_portion(menu.aspect) * p.chore.length;
        p.speed = 0.0;
        let player = ctx.commands.spawn(p).id();
        state.roots.push(player);
    }
    // Keyboard selection stays on the status button; the arrows answer to left / right.
    state.count = 1;
    state.shown = None;
}

/// `Menu_EpisodeSelect_SlideLeft` / `Right`: the next episode's setup, the diorama's slide and
/// the episode-to-episode camera move; input waits until the move is over.
fn start_slide(commands: &mut Commands, data: &GameData, diorama: &Diorama, menu: &MenuAssets, state: &mut MenuState, dir: i32) {
    if state.slide.is_some() {
        return;
    }
    let from = state.episode;
    let to = from as i32 + dir;
    if to < 1 || to > menu.episodes as i32 {
        return;
    }
    let to = to as u32;
    play_diorama(commands, data, &diorama.agents, &format!("ui_diorama_episode{to}Setup.chore"));
    play_diorama(commands, data, &diorama.agents, if dir < 0 { "ui_diorama_transitionLeft.chore" } else { "ui_diorama_transitionRight.chore" });
    let length = play_diorama(commands, data, &diorama.agents, &format!("ui_diorama_transition{from}to{to}.chore"));
    state.slide = Some((to, length.max(0.1)));
    state.rebuild = true;
}

/// Finish slides: SlideComplete plays the new episode's show chore and brings the UI back.
pub fn episode_tick(
    mut commands: Commands,
    time: Res<Time>,
    data: Option<Res<GameData>>,
    diorama: Option<Res<Diorama>>,
    menu: Option<Res<MenuAssets>>,
    mut state: ResMut<MenuState>,
    mut debug_slides: Local<u32>,
) {
    let (Some(data), Some(diorama)) = (data, diorama) else { return };
    // MCSM_DEBUG_SLIDES=n: slide right n times, one every 3 seconds (screenshots).
    let wanted: u32 = std::env::var("MCSM_DEBUG_SLIDES").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    if *debug_slides < wanted && state.slide.is_none() && time.elapsed_secs() > 3.0 * (*debug_slides + 1) as f32 {
        if let Some(menu) = menu.as_ref() {
            *debug_slides += 1;
            start_slide(&mut commands, &data, &diorama, menu, &mut state, 1);
        }
    }
    let Some((to, left)) = state.slide else { return };
    let left = left - time.delta_secs().min(1.0 / 30.0);
    if left > 0.0 {
        state.slide = Some((to, left));
        return;
    }
    state.slide = None;
    state.episode = to;
    play_diorama(&mut commands, &data, &diorama.agents, &format!("ui_diorama_episode{to}Show.chore"));
    state.rebuild = true;
}


/// `UI_Confirm`: the popup scene (window, dimming fill, header, message) with two popup
/// buttons laid out from the bottom of `ui_popup_reference`, centered.
fn build_popup(ctx: &mut Ctx, menu: &MenuAssets, state: &mut MenuState) {
    let Some(scene) = &menu.popup else { return };
    let data = ctx.data;
    let layers = RenderLayers::layer(POPUP_LAYER);
    let spawned = stage::spawn_scene(ctx, scene, POPUP_LAYER, 2, Shading::Ui, |n| n.starts_with("ui_popupButton") || n == "ui_popup_legend");
    state.popup_camera = spawned.camera;
    // Every agent of the popup goes when the page is rebuilt (children despawn with parents).
    state.roots.extend(spawned.agents.values().copied());
    let Some(&reference) = spawned.agents.get("ui_popup_reference") else { return };
    if let Some(header) = menu.text("popup_settingsconfirm_header") {
        spawn_label(ctx, scene, &spawned.agents, "ui_popup_header", &header, &layers);
    }
    if let (Some(message), Some(&entity)) = (menu.text("popup_settingsconfirm_message"), spawned.agents.get("ui_popup_message")) {
        let props = agent_props(scene, "ui_popup_message");
        if let Some(style) = TextStyle::from_props(ctx.props, data, &props) {
            let wrap = ctx.props.float(data, &props, "Text Width").filter(|w| *w > 0.0).unwrap_or(7.0);
            text::spawn_text_block(ctx, entity, &message, &style, &layers, wrap, false);
        }
    }
    let (rmin, rmax) = extents(ctx.props, data, &agent_props(scene, "ui_popup_reference")).unwrap_or((Vec2::new(-3.8, -2.0), Vec2::new(3.8, 0.15)));
    let Some(template) = menu.templates.get(&Widget::Popup) else { return };
    let ext = extents(ctx.props, data, &agent_props(template, "ui_popupButton_reference")).unwrap_or((Vec2::new(-3.6, -0.45), Vec2::new(3.65, 0.45)));
    let buttons = [("label_apply", Action::Apply), ("label_restore", Action::Restore)];
    // alignV bottom: the stack's total height sits on the bottom edge.
    let mut cursor_y = rmin.y + (ext.1.y - ext.0.y) * buttons.len() as f32;
    let center_x = (rmin.x + rmax.x) * 0.5 - (ext.0.x + ext.1.x) * 0.5;
    for (index, (label, action)) in buttons.into_iter().enumerate() {
        let Some((w_root, agents)) = spawn_widget(ctx, template, "ui_popupButton_reference", POPUP_LAYER, reference) else { continue };
        let pos = Vec3::new(center_x, cursor_y - ext.1.y, 0.0);
        cursor_y = pos.y + ext.0.y;
        ctx.commands.entity(w_root).insert(Transform::from_translation(pos));
        let text = menu.text(label).unwrap_or_default();
        let slot = spawn_label(ctx, template, &agents, "ui_popupButton_label", &text, &layers);
        ctx.commands.entity(w_root).insert(MenuItem { index, kind: Kind::Button(action), widget: Widget::Popup, extents: ext, agents, text: slot, tooltip: None, handle: None });
    }
    state.count = 2;
    state.shown = None;
}

/// Keyboard and mouse, like the game's list navigation and widget input handlers.
#[allow(clippy::too_many_arguments)]
pub fn input(
    mut commands: Commands,
    data: Option<Res<GameData>>,
    menu: Option<Res<MenuAssets>>,
    mut settings: Option<ResMut<Settings>>,
    sfx: Option<Res<crate::audio::MenuSfx>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    items: Query<(&MenuItem, &GlobalTransform)>,
    legend_back: Query<&LegendBack>,
    diorama: Option<Res<Diorama>>,
    mut state: ResMut<MenuState>,
    mut status: Query<&mut Text, With<StatusLine>>,
    mut game_cursor: Option<ResMut<crate::cursor::GameCursor>>,
    mut exit: MessageWriter<AppExit>,
) {
    let (Some(data), Some(menu), Some(settings)) = (data, menu, settings.as_mut()) else { return };
    if state.count == 0 || state.rebuild || state.transition.is_some() {
        return;
    }
    let popup = state.top() == Page::ConfirmSettings;
    // Items of the current page (popup buttons while the popup is up).
    let current = |w: Widget| (w == Widget::Popup) == popup;
    let count = state.count;
    // The appearance buttons are a 3 x 2 grid: up / down change row.
    let row = if state.top() == Page::CharacterSelect { 3.min(count) } else { 1 };
    if keys.just_pressed(KeyCode::ArrowDown) || keys.just_pressed(KeyCode::KeyS) {
        let s = (state.selected() + row) % count;
        state.select(s);
    }
    if keys.just_pressed(KeyCode::ArrowUp) || keys.just_pressed(KeyCode::KeyW) {
        let s = (state.selected() + count - row) % count;
        state.select(s);
    }
    let mut activate = keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space);
    let mut back = keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::Backspace) || mouse.just_pressed(MouseButton::Right);
    let mut adjust = 0i32;
    if keys.just_pressed(KeyCode::ArrowRight) || keys.just_pressed(KeyCode::KeyD) {
        adjust = 1;
    }
    if keys.just_pressed(KeyCode::ArrowLeft) || keys.just_pressed(KeyCode::KeyA) {
        adjust = -1;
    }
    let mut slider_set: Option<f32> = None;
    if state.top() == Page::CharacterSelect && adjust != 0 {
        let s = (state.selected() as i32 + adjust).rem_euclid(count as i32) as usize;
        state.select(s);
        adjust = 0;
    }
    if state.top() == Page::EpisodeSelect {
        if state.slide.is_some() {
            return;
        }
        if adjust != 0 {
            if let Some(d) = diorama.as_ref() {
                start_slide(&mut commands, &data, d, &menu, &mut state, adjust);
            }
            return;
        }
    }

    // Mouse: ray from the page's camera onto the menu plane (z = 0), tested against each widget.
    let cam = if popup { state.popup_camera } else { state.camera };
    if !mouse.pressed(MouseButton::Left) {
        state.dragging = None;
    }
    if let (Ok(window), Some(cam)) = (windows.single(), cam) {
        if let (Some(cursor), Ok((camera, cam_tf))) = (window.cursor_position(), cameras.get(cam)) {
            if let Ok(ray) = camera.viewport_to_world(cam_tf, cursor) {
                if let Some(t) = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Z)) {
                    let hit = ray.get_point(t);
                    let mut over = false;
                    for (item, tf) in items.iter().filter(|(i, _)| current(i.widget)) {
                        let local = tf.affine().inverse().transform_point3(hit);
                        let inside = local.x >= item.extents.0.x && local.x <= item.extents.1.x && local.y >= item.extents.0.y && local.y <= item.extents.1.y;
                        over |= inside;
                        let dragging = state.dragging == Some(item.index);
                        if inside && state.dragging.is_none() {
                            state.select(item.index);
                            if mouse.just_pressed(MouseButton::Left) {
                                if item.handle.is_some() {
                                    state.dragging = Some(item.index);
                                } else {
                                    activate = true;
                                }
                            }
                        }
                        if (inside && mouse.just_pressed(MouseButton::Left) || dragging) && item.handle.is_some() {
                            if let Some((_, l, r, _)) = item.handle {
                                slider_set = Some(((local.x - l) / (r - l)).clamp(0.0, 1.0));
                            }
                        }
                    }
                    for lb in &legend_back {
                        if !popup && hit.x >= lb.0.x && hit.x <= lb.1.x && hit.y >= lb.0.y && hit.y <= lb.1.y {
                            over = true;
                            if mouse.just_pressed(MouseButton::Left) {
                                back = true;
                            }
                        }
                    }
                    // Cursor_Highlight over anything clickable.
                    if let Some(c) = game_cursor.as_mut() {
                        if c.highlighted != over {
                            c.highlighted = over;
                        }
                    }
                }
            }
        }
    }

    let selected = state.selected();
    let Some((item, _)) = items.iter().find(|(i, _)| current(i.widget) && i.index == selected) else { return };
    let play_press = |commands: &mut Commands| {
        if let Some(c) = menu.chores.get(&item.widget).and_then(|c| c.press.clone()) {
            commands.spawn((ChorePlayer::new(&data, c, item.agents.clone(), false), DespawnWhenDone));
        }
    };

    // Value widgets: toggles cycle (activate = +1), sliders step or follow the mouse.
    let mut changed = false;
    match &item.kind {
        Kind::Toggle { key, values, .. } if adjust != 0 || activate => {
            let cur = settings.get(key);
            let i = values.iter().position(|v| *v == cur).unwrap_or(0) as i32;
            let n = values.len() as i32;
            let step = if adjust != 0 { adjust } else { 1 };
            settings.set(key, values[((i + step) % n + n) as usize % n as usize]);
            if activate {
                play_press(&mut commands);
            }
            if let Some(sfx) = sfx.as_ref() {
                sfx.play_opt(&mut commands, if step > 0 { &sfx.slider_increase } else { &sfx.slider_decrease });
            }
            changed = true;
            activate = false;
        }
        Kind::Slider { key, step, resolutions, .. } if adjust != 0 || slider_set.is_some() => {
            let before = settings.get(key);
            match resolutions {
                Some(r) if !r.is_empty() => {
                    let cur = match before {
                        SettingValue::Size(w, h) => r.iter().position(|&s| s == (w, h)).unwrap_or(0) as i32,
                        _ => 0,
                    };
                    let i = match slider_set {
                        Some(p) => (p * (r.len() - 1) as f32 + 0.5).floor() as i32,
                        None => cur + adjust,
                    }
                    .clamp(0, r.len() as i32 - 1) as usize;
                    settings.set(key, SettingValue::Size(r[i].0, r[i].1));
                }
                _ => {
                    let v = match slider_set {
                        Some(p) => p,
                        None => slider_value(before) + *step * adjust as f32,
                    };
                    // Keep keyboard steps on exact tenths.
                    let v = if slider_set.is_none() { (v * 10.0).round() / 10.0 } else { v };
                    settings.set(key, SettingValue::Float(v.clamp(0.0, 1.0)));
                }
            }
            let after = settings.get(key);
            if after != before {
                if let Some(sfx) = sfx.as_ref() {
                    let up = slider_value(after) > slider_value(before) || matches!((before, after), (SettingValue::Size(a, _), SettingValue::Size(b, _)) if b > a);
                    sfx.play_opt(&mut commands, if up { &sfx.slider_increase } else { &sfx.slider_decrease });
                }
                changed = true;
            }
            activate = false;
        }
        _ => {}
    }
    if changed {
        let i = item.index;
        if !state.changed.contains(&i) {
            state.changed.push(i);
        }
    }

    if activate {
        if let Kind::Button(action) = &item.kind {
            play_press(&mut commands);
            if let Some(sfx) = sfx.as_ref() {
                sfx.play(&mut commands, &sfx.press);
            }
            match action {
                Action::Exit => {
                    exit.write(AppExit::Success);
                }
                // Menu_Main_Play: character select until a look has been chosen, then episodes.
                Action::Open(Page::EpisodeSelect) if state.player_choice.is_none() => begin_transition(&data, &mut state, Nav::ToCharacterSelect),
                Action::Open(Page::EpisodeSelect) => begin_transition(&data, &mut state, Nav::ToEpisodes { replace: false }),
                // Menu_CharacterSelect_Complete: remember the look and continue to the episodes.
                Action::ChooseAppearance(choice) => {
                    state.player_choice = Some(*choice);
                    begin_transition(&data, &mut state, Nav::ToEpisodes { replace: true });
                }
                Action::Open(page) => state.push(*page),
                Action::SlideLeft | Action::SlideRight => {
                    if let Some(d) = diorama.as_ref() {
                        let dir = if *action == Action::SlideLeft { -1 } else { 1 };
                        start_slide(&mut commands, &data, d, &menu, &mut state, dir);
                    }
                }
                Action::StartEpisode => {
                    for mut t in &mut status {
                        t.0 = format!("Episode {} can't be played yet.", state.episode);
                    }
                }
                Action::Apply => {
                    settings.commit();
                    state.pop();
                    state.pop();
                }
                Action::Restore => {
                    settings.revert();
                    state.pop();
                    state.pop();
                }
                other => {
                    for mut t in &mut status {
                        t.0 = format!("{other:?} is not implemented yet.");
                    }
                }
            }
        }
        return;
    }

    if back {
        if let Some(sfx) = sfx.as_ref() {
            sfx.play_opt(&mut commands, &sfx.back);
        }
        match state.top() {
            Page::Main => {}
            // Escape on the popup closes it and stays in Settings.
            Page::ConfirmSettings => state.pop(),
            // Options_ExitSettings: confirm staged changes before leaving.
            Page::Settings if settings.is_dirty() => state.push(Page::ConfirmSettings),
            Page::Settings => {
                settings.revert();
                state.pop();
            }
            // Leaving a page with its own background fades.
            Page::EpisodeSelect | Page::CharacterSelect => begin_transition(&data, &mut state, Nav::Back),
            _ => state.pop(),
        }
    }
}

/// When the selection changes, play the widget's deselect chore on the old item and its
/// select chore on the new one, with the select sound; and show the new item's tooltip.
#[allow(clippy::too_many_arguments)]
pub fn apply_selection(
    mut commands: Commands,
    data: Option<Res<GameData>>,
    menu: Option<Res<MenuAssets>>,
    sfx: Option<Res<crate::audio::MenuSfx>>,
    mut state: ResMut<MenuState>,
    items: Query<&MenuItem>,
    mut tooltips: Query<&mut TooltipLine>,
    mut assets: SceneAssets,
) {
    let (Some(data), Some(menu)) = (data, menu) else { return };
    if state.count == 0 || state.rebuild || state.shown == Some(state.selected()) {
        return;
    }
    let popup = state.top() == Page::ConfirmSettings;
    let selected = state.selected();
    for item in items.iter().filter(|i| (i.widget == Widget::Popup) == popup) {
        let chores = menu.chores.get(&item.widget);
        let c = if item.index == selected {
            chores.and_then(|c| c.select.clone())
        } else if Some(item.index) == state.shown {
            chores.and_then(|c| c.deselect.clone())
        } else {
            None
        };
        if let Some(c) = c {
            commands.spawn((ChorePlayer::new(&data, c, item.agents.clone(), false), DespawnWhenDone));
        }
    }
    if state.shown.is_some() {
        if let Some(sfx) = sfx.as_ref() {
            sfx.play(&mut commands, &sfx.select);
        }
    }
    state.shown = Some(selected);

    let tip = items.iter().find(|i| (i.widget == Widget::Popup) == popup && i.index == selected).and_then(|i| i.tooltip.clone());
    let data = data.clone();
    let mut ctx = assets.ctx(&mut commands, &data);
    let layers = RenderLayers::layer(MENU_LAYER);
    for mut line in &mut tooltips {
        if line.shown == Some(selected) {
            continue;
        }
        line.shown = Some(selected);
        ctx.commands.entity(line.anchor).despawn_related::<Children>();
        if let Some(tip) = &tip {
            text::spawn_text_block(&mut ctx, line.anchor, tip, &line.style, &layers, TOOLTIP_WIDTH, true);
        }
    }
}

/// Rebuild the labels and handle positions of widgets whose values changed.
pub fn refresh(
    mut commands: Commands,
    data: Option<Res<GameData>>,
    settings: Option<Res<Settings>>,
    mut state: ResMut<MenuState>,
    mut items: Query<&mut MenuItem>,
    agent_materials: Query<&AgentMaterials>,
    mut transforms: Query<&mut Transform>,
    mut assets: SceneAssets,
) {
    let (Some(data), Some(settings)) = (data, settings) else { return };
    if state.changed.is_empty() {
        return;
    }
    let changed = std::mem::take(&mut state.changed);
    let popup = state.top() == Page::ConfirmSettings;
    let data = data.clone();
    let layers = RenderLayers::layer(MENU_LAYER);
    for mut item in items.iter_mut().filter(|i| (i.widget == Widget::Popup) == popup && changed.contains(&i.index)) {
        if let Some((e, l, r, _)) = item.handle {
            if let Ok(mut tf) = transforms.get_mut(e) {
                tf.translation.x = l + (r - l) * slider_portion(&item.kind, &settings);
            }
        }
        let (Some(slot), Some(text)) = (item.text.clone(), value_text(&item.kind, &settings)) else { continue };
        let color = agent_materials.get(slot.agent).ok().and_then(|m| m.text_color);
        let mut ctx = assets.ctx(&mut commands, &data);
        ctx.commands.entity(slot.anchor).despawn();
        let anchor = ctx.commands.spawn((Transform::IDENTITY, Visibility::Inherited, layers.clone(), ChildOf(slot.agent))).id();
        let (_, mats) = text::spawn_text(&mut ctx, anchor, &text, &slot.style, &layers);
        if let Some(c) = color {
            for h in &mats {
                if let Some(mut m) = assets.materials.get_mut(h) {
                    m.base_color = Color::srgba(c[0], c[1], c[2], c[3]);
                }
            }
        }
        commands.entity(slot.agent).entry::<AgentMaterials>().or_default().and_modify(move |mut m| m.text = mats);
        item.text = Some(TextSlot { anchor, ..slot });
    }
}
