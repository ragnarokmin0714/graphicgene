//! Tests for the invariants the rest of the project is built on.

use graphicgene_core::color::LinearRgba;
use graphicgene_core::command::{Command, Journal};
use graphicgene_core::doc::Document;
use graphicgene_core::geom::{Affine, Rect, Shape};
use graphicgene_core::node::Node;
use graphicgene_core::project::Project;

fn rect_node(name: &str) -> Node {
    Node::vector(
        name,
        Rect::new(0.0, 0.0, 10.0, 10.0).to_path(0.1),
        Some(LinearRgba::BLACK),
    )
}

fn insert(doc: &mut Document, journal: &mut Journal, node: Node) -> graphicgene_core::NodeId {
    let parent = doc.root();
    let index = doc.children_of(parent).unwrap().len();
    journal
        .execute(
            doc,
            Command::InsertNode {
                parent,
                index,
                node,
            },
        )
        .unwrap();
    *doc.children_of(parent).unwrap().last().unwrap()
}

#[test]
fn undo_preserves_node_identity() {
    // The reason undo detaches instead of deleting: a node's id must survive
    // an undo/redo cycle, or references to it (components, selection,
    // collaboration) break.
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let id = insert(&mut doc, &mut journal, rect_node("Rect"));

    assert!(journal.undo(&mut doc).unwrap());
    assert!(doc.children_of(doc.root()).unwrap().is_empty());
    assert!(doc.contains(id), "id must stay valid while detached");

    assert!(journal.redo(&mut doc).unwrap());
    assert_eq!(doc.children_of(doc.root()).unwrap(), &[id]);
}

#[test]
fn undo_redo_restores_exact_values() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let id = insert(&mut doc, &mut journal, rect_node("Rect"));

    let moved = Affine::translate((5.0, 7.0));
    journal
        .execute(
            &mut doc,
            Command::SetTransform {
                id,
                transform: moved,
            },
        )
        .unwrap();
    assert_eq!(doc.get(id).unwrap().common.transform, moved);

    journal.undo(&mut doc).unwrap();
    assert_eq!(doc.get(id).unwrap().common.transform, Affine::IDENTITY);

    journal.redo(&mut doc).unwrap();
    assert_eq!(doc.get(id).unwrap().common.transform, moved);
}

#[test]
fn a_new_edit_clears_the_redo_stack() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    insert(&mut doc, &mut journal, rect_node("A"));
    journal.undo(&mut doc).unwrap();
    assert!(journal.can_redo());

    insert(&mut doc, &mut journal, rect_node("B"));
    assert!(!journal.can_redo());
}

#[test]
fn world_transform_accumulates_through_the_tree() {
    let mut doc = Document::new();
    let mut journal = Journal::new();

    let group = insert(&mut doc, &mut journal, Node::group("Group"));
    journal
        .execute(
            &mut doc,
            Command::SetTransform {
                id: group,
                transform: Affine::translate((10.0, 0.0)),
            },
        )
        .unwrap();

    let child = doc.insert_detached(rect_node("Child"));
    doc.attach(child, group, 0).unwrap();
    doc.get_mut(child).unwrap().common.transform = Affine::translate((0.0, 5.0));

    let world = doc.world_transform(child).unwrap();
    assert_eq!(world, Affine::translate((10.0, 5.0)));

    let bounds = doc.world_bounds(child).unwrap();
    assert_eq!((bounds.x0, bounds.y0), (10.0, 5.0));
}

#[test]
fn project_round_trips_through_json() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let id = insert(&mut doc, &mut journal, rect_node("Rect"));

    let text = Project::new(doc.clone()).to_json().unwrap();
    let restored = Project::from_json(&text).unwrap().document;

    assert_eq!(restored.len(), doc.len());
    assert!(
        restored.contains(id),
        "NodeIds must survive save/load — components and collaboration depend on it"
    );
    assert_eq!(restored.get(id).unwrap().common.name, "Rect");
}

#[test]
fn a_newer_file_version_is_refused_rather_than_misread() {
    let err = Project::from_json(r#"{"version":9999,"document":{}}"#).unwrap_err();
    assert!(
        matches!(
            err,
            graphicgene_core::CoreError::UnsupportedVersion { found: 9999, .. }
        ),
        "got {err:?}"
    );
}

#[test]
fn unknown_fields_survive_a_round_trip() {
    // Opening a project saved by a newer build and re-saving it must not
    // silently destroy what this build did not understand.
    let doc = Document::new();
    let mut value: serde_json::Value =
        serde_json::from_str(&Project::new(doc).to_json().unwrap()).unwrap();
    value["futureFeature"] = serde_json::json!({"kind": "raster"});
    let text = serde_json::to_string(&value).unwrap();

    let project = Project::from_json(&text).unwrap();
    assert!(project.unknown.contains_key("futureFeature"));
    assert!(project.to_json().unwrap().contains("futureFeature"));
}

#[test]
fn purge_only_removes_unreachable_nodes() {
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let id = insert(&mut doc, &mut journal, rect_node("Rect"));
    journal.undo(&mut doc).unwrap();

    doc.purge_unreachable();
    assert!(!doc.contains(id), "detached node should be collected");
    assert!(doc.contains(doc.root()));
}

#[test]
fn geometry_survives_save_and_load_bit_for_bit() {
    // Autosave writes and reads the project over and over; if parsing is
    // off by even one ulp, geometry drifts a little on every visit. This is
    // a real value that came back one ulp off before serde_json's
    // `float_roundtrip` feature was enabled.
    let mut doc = Document::new();
    let mut journal = Journal::new();
    let id = insert(&mut doc, &mut journal, rect_node("Drift"));
    let transform = Affine::new([
        1.0812309664456263,
        0.6272476362642245,
        -0.7526971635170695,
        1.2974771597347514,
        181.65838123843517,
        -115.07110121305769,
    ]);
    doc.get_mut(id).unwrap().common.transform = transform;

    let text = Project::new(doc).to_json().unwrap();
    let back = Project::from_json(&text).unwrap().document;
    let got = back.get(id).unwrap().common.transform.as_coeffs();
    for (a, b) in got.iter().zip(transform.as_coeffs()) {
        assert_eq!(a.to_bits(), b.to_bits(), "{a} != {b}");
    }
}
