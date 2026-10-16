use super::*;
use notercrux_domain::{
    DatabaseSchema, Filter, FilterOp, Page, PageId, PageKind, Property, PropertyKind, Sort,
    SortDirection, View,
};
use serde_json::{json, Value};
use std::collections::HashMap;

fn page(title: &str, parent: Option<PageId>, position: u32) -> Page {
    Page {
        id: PageId::new(),
        parent_id: parent,
        kind: PageKind::Page,
        title: title.into(),
        icon: None,
        position,
        created_at: 100,
        updated_at: 100,
        archived_at: None,
    }
}

fn fork(from: &WorkspaceCrdt) -> WorkspaceCrdt {
    WorkspaceCrdt::from_snapshot(&from.snapshot().unwrap()).unwrap()
}

fn sync(a: &WorkspaceCrdt, b: &WorkspaceCrdt) {
    let to_b = a.updates_since(&b.version()).unwrap();
    let to_a = b.updates_since(&a.version()).unwrap();
    b.import(&to_b).unwrap();
    a.import(&to_a).unwrap();
}

fn titles(w: &WorkspaceCrdt) -> Vec<String> {
    w.pages().into_iter().map(|p| p.title).collect()
}

fn prop(id: &str, name: &str, kind: PropertyKind) -> Property {
    Property {
        id: id.into(),
        name: name.into(),
        kind,
        options: vec![],
        relation_database: None,
    }
}

fn schema_with(props: Vec<Property>) -> DatabaseSchema {
    DatabaseSchema {
        properties: props,
        ..DatabaseSchema::default()
    }
}

// ---- pages ---------------------------------------------------------------------------------

#[test]
fn the_page_tree_round_trips_with_order_metadata_and_archive_state() {
    let a = page("A", None, 0);
    let b = page("B", None, 1);
    let mut c = page("C", Some(a.id), 0);
    c.icon = Some("📚".into());
    c.archived_at = Some(555);
    let mut db = page("DB", Some(b.id), 0);
    db.kind = PageKind::Database;
    let target = vec![a.clone(), b.clone(), c.clone(), db.clone()];

    let w = WorkspaceCrdt::new();
    assert!(w.apply_pages(&target).unwrap());
    let got = w.pages();
    assert_eq!(got.len(), 4);
    let by = |t: &str| got.iter().find(|p| p.title == t).unwrap().clone();
    assert_eq!(by("C").parent_id, Some(a.id));
    assert_eq!(by("C").icon.as_deref(), Some("📚"));
    assert_eq!(by("C").archived_at, Some(555));
    assert_eq!(by("DB").kind, PageKind::Database);
    assert_eq!((by("A").position, by("B").position), (0, 1));
    assert!(
        !w.apply_pages(&target).unwrap(),
        "re-applying the same state changes nothing"
    );

    // Reorder, rename, un-archive and delete in one step.
    let mut a2 = a.clone();
    a2.position = 1;
    let mut b2 = b.clone();
    b2.position = 0;
    let mut c2 = c.clone();
    c2.archived_at = None;
    c2.title = "C renamed".into();
    let next = vec![a2, b2, c2.clone()]; // DB deleted
    assert!(w.apply_pages(&next).unwrap());
    assert_eq!(titles(&w), ["B", "A", "C renamed"]);
    assert_eq!(
        w.pages().iter().find(|p| p.id == c.id).unwrap().archived_at,
        None
    );
    assert!(
        WorkspaceCrdt::from_snapshot(&w.snapshot().unwrap())
            .unwrap()
            .pages()
            .len()
            == 3
    );
}

#[test]
fn renames_moves_and_creations_on_two_devices_merge() {
    let w = WorkspaceCrdt::new();
    let (p, q, r) = (page("P", None, 0), page("Q", None, 1), page("R", None, 2));
    w.apply_pages(&[p.clone(), q.clone(), r.clone()]).unwrap();
    let a = fork(&w);
    let b = fork(&w);

    // A renames P and creates a page; B moves R under Q and creates a page.
    let mut p2 = p.clone();
    p2.title = "P renamed".into();
    a.set_page(&p2).unwrap();
    let on_a = page("from A", None, 3);
    a.set_page(&on_a).unwrap();
    let mut r2 = r.clone();
    r2.parent_id = Some(q.id);
    r2.position = 0;
    b.apply_pages(&[p.clone(), q.clone(), r2, page("from B", None, 2)])
        .unwrap();

    sync(&a, &b);
    assert_eq!(a.pages(), b.pages());
    let t = titles(&a);
    for want in ["P renamed", "Q", "R", "from A", "from B"] {
        assert!(t.contains(&want.to_string()), "lost {want}: {t:?}");
    }
    assert_eq!(
        a.pages().iter().find(|x| x.id == r.id).unwrap().parent_id,
        Some(q.id)
    );
}

#[test]
fn concurrent_moves_forming_a_cycle_never_corrupt_the_tree_or_lose_pages() {
    let w = WorkspaceCrdt::new();
    let (p, q) = (page("P", None, 0), page("Q", None, 1));
    w.apply_pages(&[p.clone(), q.clone()]).unwrap();
    let (a, b) = (fork(&w), fork(&w));
    let mut p_under_q = p.clone();
    p_under_q.parent_id = Some(q.id);
    p_under_q.position = 0;
    a.apply_pages(&[p_under_q, q.clone()]).unwrap();
    let mut q_under_p = q.clone();
    q_under_p.parent_id = Some(p.id);
    q_under_p.position = 0;
    b.apply_pages(&[p.clone(), q_under_p]).unwrap();
    sync(&a, &b);
    assert_eq!(a.pages(), b.pages());
    assert_eq!(a.pages().len(), 2, "both pages are still there");
}

#[test]
fn a_page_deleted_on_one_device_and_renamed_on_another_is_deleted_everywhere() {
    let w = WorkspaceCrdt::new();
    let (p, q) = (page("P", None, 0), page("Q", None, 1));
    w.apply_pages(&[p.clone(), q.clone()]).unwrap();
    let (a, b) = (fork(&w), fork(&w));
    let mut q2 = q.clone();
    q2.position = 0;
    a.apply_pages(&[q2]).unwrap(); // deletes P
    let mut p2 = p.clone();
    p2.title = "P edited".into();
    b.set_page(&p2).unwrap();
    sync(&a, &b);
    assert_eq!(a.pages(), b.pages());
    assert_eq!(titles(&a), ["Q"]);
}

// ---- schemas -------------------------------------------------------------------------------

#[test]
fn schemas_round_trip_with_order_views_and_properties() {
    let db = PageId::new();
    let mut s = schema_with(vec![
        prop("a", "A", PropertyKind::Text),
        prop("b", "B", PropertyKind::Number),
        prop("c", "C", PropertyKind::Date),
    ]);
    s.views.push(View {
        id: "v2".into(),
        name: "Open".into(),
        sorts: vec![Sort {
            property: "b".into(),
            direction: SortDirection::Desc,
        }],
        filters: vec![Filter {
            property: "a".into(),
            op: FilterOp::Contains,
            value: Some(json!("x")),
        }],
        filter_mode: Default::default(),
        hidden: vec!["c".into()],
    });
    let w = WorkspaceCrdt::new();
    w.set_schema(db, &s).unwrap();
    assert_eq!(w.schemas()[&db], s);
    s.properties.swap(0, 2); // reorder
    s.properties.remove(1); // delete B (and the view's sort on it must be cleaned by the caller)
    s.views[1].sorts.clear();
    w.set_schema(db, &s).unwrap();
    assert_eq!(w.schemas()[&db], s);
    assert!(w.schemas()[&db].validate().is_ok());
}

#[test]
fn concurrent_schema_edits_to_different_properties_both_survive() {
    let db = PageId::new();
    let w = WorkspaceCrdt::new();
    w.set_schema(db, &schema_with(vec![prop("a", "A", PropertyKind::Text)]))
        .unwrap();
    let (x, y) = (fork(&w), fork(&w));
    // X adds property "x" and renames "a"; Y adds property "y".
    let mut sx = x.schemas()[&db].clone();
    sx.properties[0].name = "A renamed".into();
    sx.properties.push(prop("x", "X", PropertyKind::Number));
    x.set_schema(db, &sx).unwrap();
    let mut sy = y.schemas()[&db].clone();
    sy.properties.push(prop("y", "Y", PropertyKind::Date));
    y.set_schema(db, &sy).unwrap();
    sync(&x, &y);
    let merged = x.schemas()[&db].clone();
    assert_eq!(merged, y.schemas()[&db]);
    let ids: Vec<&str> = merged.properties.iter().map(|p| p.id.as_str()).collect();
    assert!(
        ids.contains(&"a") && ids.contains(&"x") && ids.contains(&"y"),
        "{ids:?}"
    );
    assert_eq!(
        merged.properties.iter().find(|p| p.id == "a").unwrap().name,
        "A renamed"
    );
    assert!(merged.validate().is_ok());
}

#[test]
fn a_property_deleted_on_one_device_is_removed_from_views_edited_on_another() {
    let db = PageId::new();
    let w = WorkspaceCrdt::new();
    w.set_schema(
        db,
        &schema_with(vec![
            prop("a", "A", PropertyKind::Text),
            prop("b", "B", PropertyKind::Number),
        ]),
    )
    .unwrap();
    let (x, y) = (fork(&w), fork(&w));
    let mut sx = x.schemas()[&db].clone();
    sx.properties.retain(|p| p.id != "b"); // X deletes B
    x.set_schema(db, &sx).unwrap();
    let mut sy = y.schemas()[&db].clone();
    sy.views[0].sorts.push(Sort {
        property: "b".into(),
        direction: SortDirection::Asc,
    }); // Y sorts by B
    y.set_schema(db, &sy).unwrap();
    sync(&x, &y);
    let merged = x.schemas()[&db].clone();
    assert_eq!(merged, y.schemas()[&db]);
    assert!(merged.properties.iter().all(|p| p.id != "b"));
    assert!(
        merged.views[0].sorts.is_empty(),
        "the dangling sort is repaired away"
    );
    assert!(merged.validate().is_ok(), "{:?}", merged.validate());
}

// ---- values --------------------------------------------------------------------------------

#[test]
fn first_values_for_the_same_record_set_concurrently_on_two_devices_both_survive() {
    let rec = PageId::new();
    let w = WorkspaceCrdt::new();
    let (a, b) = (fork(&w), fork(&w));
    a.set_value(rec, "status", Some(&json!("done"))).unwrap();
    b.set_value(rec, "points", Some(&json!(8))).unwrap();
    sync(&a, &b);
    let expect: HashMap<String, Value> = [
        ("status".into(), json!("done")),
        ("points".into(), json!(8)),
    ]
    .into();
    assert_eq!(
        a.values()[&rec],
        expect,
        "neither device's first edit may be lost"
    );
    assert_eq!(a.values(), b.values());
}

#[test]
fn the_same_cell_edited_on_two_devices_converges_to_one_value_and_clears_work() {
    let rec = PageId::new();
    let w = WorkspaceCrdt::new();
    w.set_value(rec, "n", Some(&json!(1))).unwrap();
    let (a, b) = (fork(&w), fork(&w));
    a.set_value(rec, "n", Some(&json!(2))).unwrap();
    b.set_value(rec, "n", Some(&json!(3))).unwrap();
    sync(&a, &b);
    assert_eq!(a.values(), b.values());
    assert!([json!(2), json!(3)].contains(&a.values()[&rec]["n"]));
    a.set_value(rec, "n", None).unwrap();
    sync(&a, &b);
    assert!(!b.values()[&rec].contains_key("n"));
}

#[test]
fn record_values_replace_cleanly() {
    let rec = PageId::new();
    let w = WorkspaceCrdt::new();
    w.set_record_values(
        rec,
        &[("a".to_string(), json!(1)), ("b".to_string(), json!("x"))].into(),
    )
    .unwrap();
    w.set_record_values(rec, &[("b".to_string(), json!("y"))].into())
        .unwrap();
    assert_eq!(
        w.values()[&rec],
        HashMap::from([("b".to_string(), json!("y"))])
    );
}

// ---- randomized convergence ----------------------------------------------------------------

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

#[test]
fn random_concurrent_workspace_editing_always_converges() {
    for seed in 1..=10u64 {
        let mut rng = Rng(0xA076_1D64_78BD_642F ^ seed.wrapping_mul(0xE703_7ED1_A0B4_28DB));
        let base = WorkspaceCrdt::new();
        let root = page("root", None, 0);
        base.apply_pages(std::slice::from_ref(&root)).unwrap();
        let db = PageId::new();
        base.set_schema(db, &schema_with(vec![prop("p0", "P0", PropertyKind::Text)]))
            .unwrap();
        let reps = [fork(&base), fork(&base), fork(&base)];
        let mut n = 0;

        for _ in 0..150 {
            let r = &reps[rng.below(3)];
            n += 1;
            let mut pages = r.pages();
            match rng.below(6) {
                0 => {
                    let parent = if pages.is_empty() || rng.below(3) == 0 {
                        None
                    } else {
                        Some(pages[rng.below(pages.len())].id)
                    };
                    let pos = pages.iter().filter(|p| p.parent_id == parent).count() as u32;
                    pages.push(page(&format!("page {n}"), parent, pos));
                    r.apply_pages(&pages).unwrap();
                }
                1 if !pages.is_empty() => {
                    let i = rng.below(pages.len());
                    pages[i].title = format!("renamed {n}");
                    r.set_page(&pages[i].clone()).unwrap();
                }
                2 if pages.len() > 2 => {
                    let i = rng.below(pages.len());
                    let target = pages[rng.below(pages.len())].id;
                    // Move page i under `target` unless that would create a cycle locally.
                    let mut cur = Some(target);
                    let mut cyc = false;
                    while let Some(c) = cur {
                        if c == pages[i].id {
                            cyc = true;
                            break;
                        }
                        cur = pages.iter().find(|p| p.id == c).and_then(|p| p.parent_id);
                    }
                    if !cyc {
                        pages[i].parent_id = Some(target);
                        pages[i].position = 0;
                        let id = pages[i].id;
                        // Renumber siblings so positions stay dense.
                        let mut sib: Vec<usize> = (0..pages.len())
                            .filter(|&k| pages[k].parent_id == Some(target) && pages[k].id != id)
                            .collect();
                        sib.sort_by_key(|&k| pages[k].position);
                        for (pos, k) in sib.into_iter().enumerate() {
                            pages[k].position = pos as u32 + 1;
                        }
                        r.apply_pages(&pages).unwrap();
                    }
                }
                3 if pages.len() > 3 => {
                    let i = rng.below(pages.len());
                    let gone: Vec<PageId> = {
                        let mut set = vec![pages[i].id];
                        let mut grew = true;
                        while grew {
                            grew = false;
                            for p in &pages {
                                if p.parent_id.is_some_and(|x| set.contains(&x))
                                    && !set.contains(&p.id)
                                {
                                    set.push(p.id);
                                    grew = true;
                                }
                            }
                        }
                        set
                    };
                    pages.retain(|p| !gone.contains(&p.id));
                    r.apply_pages(&pages).unwrap();
                }
                4 => {
                    let rec = if pages.is_empty() {
                        PageId::new()
                    } else {
                        pages[rng.below(pages.len())].id
                    };
                    let prop_id = format!("p{}", rng.below(3));
                    if rng.below(4) == 0 {
                        r.set_value(rec, &prop_id, None).unwrap();
                    } else {
                        r.set_value(rec, &prop_id, Some(&json!(format!("v{n}"))))
                            .unwrap();
                    }
                }
                _ => {
                    let mut s = r.schemas().get(&db).cloned().unwrap_or_default();
                    if rng.below(2) == 0 || s.properties.is_empty() {
                        s.properties
                            .push(prop(&format!("q{n}"), "Q", PropertyKind::Text));
                    } else {
                        let i = rng.below(s.properties.len());
                        s.properties[i].name = format!("name {n}");
                    }
                    r.set_schema(db, &s).unwrap();
                }
            }
            if rng.below(4) == 0 {
                let (i, j) = (rng.below(3), rng.below(3));
                if i != j {
                    let u = reps[i].updates_since(&reps[j].version()).unwrap();
                    reps[j].import(&u).unwrap();
                }
            }
        }
        for _ in 0..2 {
            for i in 0..3 {
                for j in 0..3 {
                    if i != j {
                        let u = reps[i].updates_since(&reps[j].version()).unwrap();
                        reps[j].import(&u).unwrap();
                    }
                }
            }
        }
        assert_eq!(
            reps[0].pages(),
            reps[1].pages(),
            "seed {seed}: page trees diverged"
        );
        assert_eq!(
            reps[0].pages(),
            reps[2].pages(),
            "seed {seed}: page trees diverged"
        );
        assert_eq!(
            reps[0].schemas(),
            reps[1].schemas(),
            "seed {seed}: schemas diverged"
        );
        assert_eq!(
            reps[0].values(),
            reps[2].values(),
            "seed {seed}: values diverged"
        );
        // Positions are dense per parent and ids unique.
        let pages = reps[0].pages();
        let mut ids: Vec<_> = pages.iter().map(|p| p.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), pages.len(), "seed {seed}: duplicate page ids");
        assert!(
            reps[0].schemas().values().all(|s| s.validate().is_ok()),
            "seed {seed}: invalid merged schema"
        );
    }
}
