//! Optional structural tests of the opening story data; no game contents in fixtures.
use ttarch::Install;
use ttmeta::{dialog::{Dialog, NodeKind}, Chore};

#[test]
fn opening_dialogues_and_embedded_timelines() {
    let Some(dir) = std::env::var_os("MCSM_GAME_DIR") else { return };
    let vfs = Install::open(dir.as_ref()).unwrap().mount().unwrap();
    let read = |name: &str| vfs.read(name).unwrap().unwrap();
    let intro = Dialog::parse(&read("env_theEnd.dlog")).unwrap();
    let start = intro.named("cs_orderOfTheStone").unwrap();
    let script = &intro.nodes[&start.next];
    assert!(matches!(script.kind, NodeKind::Script(_)));
    assert!(matches!(intro.nodes[&script.next].kind, NodeKind::Exchange(_)));
    let chore = Chore::parse(&read("env_theEnd_cs_orderOfTheStone_2.chore")).unwrap();
    assert!(chore.length > 0.0);
    assert!(chore.resources.iter().any(|r| r.embedded_chore.is_some()));
    let treehouse = Dialog::parse(&read("env_forestTreehouse.dlog")).unwrap();
    assert!(treehouse.named("cs_treeHouse").is_some());
    assert!(treehouse.nodes.values().any(|n| matches!(&n.kind, NodeKind::Choices { choices, .. } if choices.len() == 4)));
    assert!(treehouse.nodes.values().any(|n| matches!(&n.kind, NodeKind::Assign(groups) if groups.iter().any(|(_, p)| !p.values.is_empty()))));
}

/// Every dialogue file in the install: how many index without an error (not all node types are
/// supported yet, so this reports rather than requires a full pass).
#[test]
fn dialogue_sweep() {
    let Some(dir) = std::env::var_os("MCSM_GAME_DIR") else { return };
    let vfs = Install::open(dir.as_ref()).unwrap().mount().unwrap();
    let mut names: Vec<String> = vfs.archives.iter().flat_map(|a| a.entries().iter().map(|e| e.name.clone())).filter(|n| n.to_ascii_lowercase().ends_with(".dlog")).collect();
    names.sort();
    names.dedup();
    let mut errors = std::collections::BTreeMap::<String, usize>::new();
    let mut ok = 0;
    for name in &names {
        match Dialog::parse(&vfs.read(name).unwrap().unwrap()) {
            Ok(_) => ok += 1,
            Err(e) => *errors.entry(e.to_string().chars().take(90).collect()).or_default() += 1,
        }
    }
    eprintln!(".dlog: {ok} of {} index", names.len());
    for (e, n) in &errors {
        eprintln!("  {n:4}  {e}");
    }
    assert!(ok > 0);
}
