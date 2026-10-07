//! Text: fonts in, layout out, typing as one undo step. The font is made in
//! code (`testing.rs`), so every expected number below follows from it:
//! 1000 units per em, ascender 800, descender -200, and glyph boxes 700
//! tall, inset 50 units from each side of their advance.

use graphicgene_core::color::LinearRgba;
use graphicgene_core::error::CoreError;
use graphicgene_core::fonts::Fonts;
use graphicgene_core::geom::{BezPath, PathEl, Point, Rect};
use graphicgene_core::gesture::Modifiers;
use graphicgene_core::node::{NodeId, NodeKind};
use graphicgene_core::paint::Paint;
use graphicgene_core::properties::{Property, Shared};
use graphicgene_core::session::{Mode, Pointer, SelectOutcome, Session, Tool};
use graphicgene_core::testing::TestFont;
use graphicgene_core::text::{TextAlign, TextStyle, layout};

const INK: LinearRgba = LinearRgba::new(0.1, 0.2, 0.3, 1.0);

fn latin() -> TestFont {
    TestFont::new("Latin").glyphs("ABCHelo ", 600)
}

fn style(size: f64) -> TextStyle {
    TextStyle {
        family: "Latin".into(),
        size,
        ..TextStyle::default()
    }
}

/// Each glyph is one closed box; their bounds, in order.
fn glyph_boxes(path: &BezPath) -> Vec<Rect> {
    let mut boxes = Vec::new();
    let mut current: Vec<Point> = Vec::new();
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => current = vec![p],
            PathEl::LineTo(p) => current.push(p),
            PathEl::ClosePath => {
                let xs = current.iter().map(|p| p.x);
                let ys = current.iter().map(|p| p.y);
                boxes.push(Rect::new(
                    xs.clone().fold(f64::INFINITY, f64::min),
                    ys.clone().fold(f64::INFINITY, f64::min),
                    xs.fold(f64::NEG_INFINITY, f64::max),
                    ys.fold(f64::NEG_INFINITY, f64::max),
                ));
            }
            _ => {}
        }
    }
    boxes
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn fonts_are_known_by_their_family() {
    let mut fonts = Fonts::default();
    assert_eq!(fonts.add(latin().build()).unwrap(), "Latin");
    assert_eq!(
        fonts
            .add(TestFont::new("Han").glyph('中', 1000).build())
            .unwrap(),
        "Han"
    );
    assert_eq!(fonts.add(latin().glyph('Z', 600).build()).unwrap(), "Latin");
    assert_eq!(fonts.families(), ["Latin", "Han"], "in the order they came");
    assert!(matches!(
        fonts.add(b"not a font".to_vec()),
        Err(CoreError::BadFont)
    ));
}

#[test]
fn glyphs_advance_and_kern() {
    let mut fonts = Fonts::default();
    fonts.add(latin().build()).unwrap();
    let plain = layout(&fonts, "AB", &style(100.0));
    assert!(close(plain.bounds.width(), 120.0), "{:?}", plain.bounds);
    let boxes = glyph_boxes(&plain.path);
    assert!(close(boxes[0].x0, 5.0) && close(boxes[1].x0, 65.0));

    // The old kern table, and GPOS, which modern fonts use.
    for font in [
        latin().kern('A', 'B', -100),
        latin().kern('A', 'B', -100).gpos(),
    ] {
        let mut fonts = Fonts::default();
        fonts.add(font.build()).unwrap();
        let kerned = layout(&fonts, "AB", &style(100.0));
        assert!(close(kerned.bounds.width(), 110.0), "{:?}", kerned.bounds);
        assert!(close(glyph_boxes(&kerned.path)[1].x0, 55.0));
        let other = layout(&fonts, "BA", &style(100.0));
        assert!(close(other.bounds.width(), 120.0), "only the pair kerned");
    }
}

#[test]
fn lines_break_at_newlines_align_and_sit_as_css_would() {
    let mut fonts = Fonts::default();
    fonts.add(latin().build()).unwrap();
    let text = "A\nABC";
    // Line height 1.2 × 100; the 100 of ascent and descent centred in it,
    // so the first baseline is at 10 + 80.
    let left = layout(&fonts, text, &style(100.0));
    assert_eq!(left.bounds, Rect::new(0.0, 0.0, 180.0, 240.0));
    let first = glyph_boxes(&left.path)[0];
    assert!(close(first.y1, 90.0) && close(first.y0, 20.0), "{first:?}");
    let second_line = glyph_boxes(&left.path)[1];
    assert!(close(second_line.y1, 210.0), "one line height lower");

    for (align, x) in [
        (TextAlign::Left, 5.0),
        (TextAlign::Center, 65.0),
        (TextAlign::Right, 125.0),
    ] {
        let laid = layout(
            &fonts,
            text,
            &TextStyle {
                align,
                ..style(100.0)
            },
        );
        assert!(close(glyph_boxes(&laid.path)[0].x0, x), "{align:?}");
    }

    let empty = layout(&fonts, "", &style(100.0));
    assert_eq!(
        empty.bounds,
        Rect::new(0.0, 0.0, 0.0, 120.0),
        "one empty line"
    );
}

#[test]
fn characters_fall_back_to_other_families_and_the_rest_are_missing() {
    let mut fonts = Fonts::default();
    fonts.add(latin().build()).unwrap();
    // Han also has an A, wider: the text's own family still wins it.
    fonts
        .add(
            TestFont::new("Han")
                .glyph('中', 1000)
                .glyph('A', 900)
                .build(),
        )
        .unwrap();
    let laid = layout(&fonts, "中A字", &style(100.0));
    assert!(
        close(laid.bounds.width(), 160.0),
        "中 from Han, A from Latin: {:?}",
        laid.bounds
    );
    assert_eq!(glyph_boxes(&laid.path).len(), 2);
    assert_eq!(laid.missing.iter().collect::<String>(), "字");
}

fn session_with_fonts() -> Session {
    let mut session = Session::new();
    session.add_font(latin().build()).unwrap();
    session
}

fn text_of(session: &Session, id: NodeId) -> (String, String) {
    let node = session.document().get(id).unwrap();
    match &node.kind {
        NodeKind::Text(text) => (text.content.clone(), node.common.name.clone()),
        _ => panic!("not text"),
    }
}

fn count(session: &Session) -> usize {
    let doc = session.document();
    doc.children_of(doc.root()).unwrap().len()
}

#[test]
fn text_is_laid_out_again_when_fonts_arrive() {
    let mut session = Session::new();
    let id = session.begin_text(Point::new(10.0, 20.0), INK).unwrap();
    session.preview_text("AB").unwrap();
    session.prepare_render().unwrap();
    let missing = session.missing_glyphs().unwrap();
    assert_eq!(missing[""].iter().collect::<String>(), "AB", "no fonts yet");

    let version = session.glyphs_version();
    session.add_font(latin().build()).unwrap();
    session.prepare_render().unwrap();
    assert!(session.glyphs_version() > version);
    assert!(session.missing_glyphs().unwrap().is_empty());
    let frame = graphicgene_core::gesture::Frame::of(session.document(), &[id])
        .unwrap()
        .unwrap();
    assert!(
        close(frame.size().0, 28.8),
        "two 600-unit glyphs at 24: {:?}",
        frame.size()
    );

    // An idle frame lays nothing out.
    let version = session.glyphs_version();
    session.prepare_render().unwrap();
    assert_eq!(session.glyphs_version(), version);
}

#[test]
fn typing_new_text_is_one_undo_step_named_after_it() {
    let mut session = session_with_fonts();
    let id = session.begin_text(Point::new(0.0, 0.0), INK).unwrap();
    assert_eq!(session.mode(), Some(Mode::Text));
    for typed in ["H", "He", "Hel", "Hello"] {
        session.preview_text(typed).unwrap();
    }
    assert!(
        !session.busy(),
        "saved as it is typed: typing can last minutes"
    );
    assert_eq!(session.commit_text().unwrap(), Some(id));
    assert_eq!(text_of(&session, id), ("Hello".into(), "Hello".into()));
    assert_eq!(session.mode(), None);

    session.undo().unwrap();
    assert_eq!(count(&session), 0, "one step takes the whole text away");
    session.redo().unwrap();
    assert_eq!(text_of(&session, id).0, "Hello");

    // New text committed empty leaves nothing, not even a step.
    session.begin_text(Point::new(50.0, 50.0), INK).unwrap();
    session.preview_text("  ").unwrap();
    assert_eq!(session.commit_text().unwrap(), None);
    assert_eq!(count(&session), 1);
    session.undo().unwrap();
    assert_eq!(count(&session), 0, "the step undone was the first text's");
}

#[test]
fn editing_text_is_one_step_and_the_name_follows_unless_renamed() {
    let mut session = session_with_fonts();
    let id = session.begin_text(Point::new(0.0, 0.0), INK).unwrap();
    session.preview_text("Hello").unwrap();
    session.commit_text().unwrap();

    assert!(session.edit_text(id).unwrap());
    session.preview_text("Hell").unwrap();
    session.preview_text("Hello Bee").unwrap();
    session.commit_text().unwrap();
    assert_eq!(
        text_of(&session, id),
        ("Hello Bee".into(), "Hello Bee".into())
    );
    session.undo().unwrap();
    assert_eq!(
        text_of(&session, id),
        ("Hello".into(), "Hello".into()),
        "one step"
    );

    session.rename(id, "Title").unwrap();
    session.edit_text(id).unwrap();
    session.preview_text("Hola").unwrap();
    session.commit_text().unwrap();
    assert_eq!(
        text_of(&session, id),
        ("Hola".into(), "Title".into()),
        "a chosen name stays"
    );

    // Cancelled, it says what it said; emptied, it goes, as one step.
    session.edit_text(id).unwrap();
    session.preview_text("zzz").unwrap();
    session.cancel_text().unwrap();
    assert_eq!(text_of(&session, id).0, "Hola");
    session.edit_text(id).unwrap();
    session.preview_text("").unwrap();
    assert_eq!(session.commit_text().unwrap(), None);
    assert_eq!(count(&session), 0);
    session.undo().unwrap();
    assert_eq!(text_of(&session, id).0, "Hola");
}

fn at(x: f64, y: f64) -> Pointer {
    Pointer {
        point: Point::new(x, y),
        modifiers: Modifiers::default(),
        hit_tolerance: 1.0,
        pick_tolerance: 1.0,
    }
}

fn click(session: &mut Session, x: f64, y: f64) {
    session.pointer_down(at(x, y), None, INK).unwrap();
    session.pointer_up().unwrap();
}

#[test]
fn the_text_tool_types_where_it_is_pressed() {
    let mut session = session_with_fonts();
    session.set_tool(Tool::Text).unwrap();
    click(&mut session, 100.0, 100.0);
    assert_eq!((session.mode(), count(&session)), (Some(Mode::Text), 1));
    session.preview_text("Hello").unwrap();
    session.prepare_render().unwrap();

    // A press elsewhere finishes it and is a select press from then on.
    click(&mut session, 500.0, 500.0);
    assert_eq!((session.mode(), session.tool()), (None, Tool::Select));
    assert!(
        session.selection().is_empty(),
        "the press missed everything"
    );
    assert_eq!(count(&session), 1);

    // In the box, between the glyphs, still hits the text.
    let inside = Point::new(100.0 + 14.4, 110.0);
    assert_eq!(
        session.select_at(inside, false, 1.0).unwrap(),
        SelectOutcome::Drag
    );
    session.double_click(at(inside.x, inside.y)).unwrap();
    assert_eq!(
        session.mode(),
        Some(Mode::Text),
        "a double-click types into it"
    );
    session.escape().unwrap();
    assert_eq!(session.mode(), None);
    session.enter().unwrap();
    assert_eq!(session.mode(), Some(Mode::Text), "so does Enter");
    session.escape().unwrap();

    // The text tool on existing text types into it rather than adding more.
    session.set_tool(Tool::Text).unwrap();
    click(&mut session, inside.x, inside.y);
    assert_eq!((session.mode(), count(&session)), (Some(Mode::Text), 1));
    session.escape().unwrap();
    assert_eq!(
        session.tool(),
        Tool::Select,
        "Escape puts the text tool down"
    );
}

#[test]
fn the_properties_panel_sets_text() {
    let mut session = session_with_fonts();
    let id = session.begin_text(Point::new(0.0, 0.0), INK).unwrap();
    session.preview_text("Hello").unwrap();
    session.commit_text().unwrap();
    session.select_layer(id, false).unwrap();

    let p = session.properties().unwrap().unwrap();
    let text = p.text.clone().expect("text is selected");
    assert_eq!(text.family, Shared::Same("Latin".to_owned()));
    assert_eq!(
        (text.size, text.align),
        (Shared::Same(24.0), Shared::Same(TextAlign::Left))
    );
    assert_eq!(p.fill, Some(Shared::Same(Some(Paint::Solid(INK)))));
    assert_eq!(p.stroke, None, "text has no stroke");

    assert!(session.set_property(Property::FontSize(48.0)).unwrap());
    assert!(
        session
            .set_property(Property::FontFamily("Serif".into()))
            .unwrap()
    );
    assert!(
        session
            .set_property(Property::TextAlign(TextAlign::Center))
            .unwrap()
    );
    assert!(!session.set_property(Property::FontSize(f64::NAN)).unwrap());
    let text = session.properties().unwrap().unwrap().text.unwrap();
    assert_eq!(text.size, Shared::Same(48.0));
    assert_eq!(text.family, Shared::Same("Serif".to_owned()));
    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(
        session.properties().unwrap().unwrap().text.unwrap().size,
        Shared::Same(48.0)
    );
    session.undo().unwrap();
    assert_eq!(
        session.properties().unwrap().unwrap().text.unwrap().size,
        Shared::Same(24.0),
        "a step each"
    );
}

#[test]
fn text_is_saved_copied_and_exported_as_outlines() {
    let mut session = session_with_fonts();
    let id = session.begin_text(Point::new(0.0, 0.0), INK).unwrap();
    session.preview_text("Hello <&>").unwrap();
    session.commit_text().unwrap();
    session.prepare_render().unwrap();

    let svg = session.export_svg().unwrap();
    assert!(svg.contains(r#"aria-label="Hello &lt;&amp;&gt;""#), "{svg}");
    assert!(svg.contains(" d=\"M"), "the glyphs, as a path");

    let mut reopened = session_with_fonts();
    reopened.load(&session.save().unwrap()).unwrap();
    assert_eq!(text_of(&reopened, id).0, "Hello <&>");
    reopened.prepare_render().unwrap();
    assert!(
        reopened.missing_glyphs().unwrap()["Latin"].contains(&'<'),
        "laid out afresh"
    );

    session.select_layer(id, false).unwrap();
    let copied = session.copy_selection().unwrap().unwrap();
    assert!(session.paste(&copied).unwrap());
    let pasted = session.selection().ids()[0];
    assert_eq!(text_of(&session, pasted).0, "Hello <&>");
}
