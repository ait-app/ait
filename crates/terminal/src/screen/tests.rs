use super::*;

mod paseo;

#[test]
fn shrinking_through_a_wide_character_keeps_writes_and_erase_in_bounds() {
    for output in [b"\x1b[1;3HX".as_slice(), b"\x1b[1;3H\x1b[K"] {
        let mut screen = Screen::new(Size { rows: 2, cols: 4 });
        screen.process("ab界".as_bytes());

        screen.resize(Size { rows: 2, cols: 3 });
        screen.process(output);

        assert_eq!(
            screen.capture()[0],
            if output.ends_with(b"X") { "abX" } else { "ab" }
        );
        assert!(screen.observe(None, None).is_ok());
        assert!(!screen.ansi(0).is_empty());
    }
}

#[test]
fn one_cell_screens_accept_output_after_wide_characters_and_wrapping() {
    let mut screen = Screen::new(Size { rows: 1, cols: 1 });

    screen.process("界abc".as_bytes());

    assert_eq!(screen.capture(), ["a", "b", "c"]);
    assert!(screen.observe(None, None).is_ok());
}

fn screen() -> Screen {
    Screen::new(Size { rows: 3, cols: 12 })
}
fn restore(mode: RestoreMode) -> Restore {
    Restore {
        mode,
        scrollback_lines: Some(2),
        size: None,
    }
}

#[test]
fn utf8_ansi_cells_titles_cursor_and_capture_match_rendered_output() {
    let mut screen = screen();
    let text = "\x1b]2;build title\x07\x1b[1;3;4;38;2;12;34;56m你好\x1b[0m!\r\nnext".as_bytes();
    for byte in text {
        screen.process(&[*byte]);
    }
    assert_eq!(screen.title().as_deref(), Some("build title"));
    assert_eq!(screen.capture(), ["你好!", "next", ""]);
    let snapshot = screen.observe(None, None).unwrap();
    assert_eq!(snapshot.frames[0].0, Opcode::Snapshot);
    let state: Value = serde_json::from_slice(&snapshot.frames[0].1).unwrap();
    assert_eq!(state["grid"][0][0]["char"], "你");
    assert_eq!(state["grid"][0][1]["char"], "");
    assert_eq!(state["grid"][0][0]["fgMode"], 3);
    assert_eq!(state["grid"][0][0]["fg"], 0x000c_2238);
    assert_eq!(state["grid"][0][0]["bold"], true);
    assert_eq!(state["cursor"]["row"], 1);
    assert_eq!(state["cursor"]["col"], 4);
    screen.process(b"\r\x1b[2Kreplaced");
    assert_eq!(screen.capture()[1], "replaced");
}

#[test]
fn history_live_bootstrap_resize_and_overflow_have_atomic_cursors() {
    let mut screen = screen();
    for index in 0..10 {
        screen.process(format!("line{index}\r\n").as_bytes());
    }
    let initial = screen
        .observe(None, Some(&restore(RestoreMode::Live)))
        .unwrap();
    assert_eq!(initial.frames[0].0, Opcode::Restore);
    screen.process(b"tail");
    let delta = screen.observe(Some(initial.revision), None).unwrap();
    assert_eq!(delta.frames, [(Opcode::Output, b"tail".to_vec())]);
    assert!(
        screen
            .observe(Some(delta.revision), None)
            .unwrap()
            .frames
            .is_empty()
    );
    let snapshot = screen.snapshot(2);
    assert_eq!(snapshot["scrollback"].as_array().unwrap().len(), 2);
    let ansi = screen
        .observe(None, Some(&restore(RestoreMode::FullSnapshot)))
        .unwrap();
    let mut restored = Screen::new(Size { rows: 3, cols: 12 });
    restored.process(&ansi.frames[0].1);
    assert_eq!(restored.capture(), screen.capture());
    screen.resize(Size { rows: 4, cols: 15 });
    assert_eq!(
        screen.observe(Some(delta.revision), None).unwrap().frames[0].0,
        Opcode::Snapshot
    );
    for _ in 0..20 {
        screen.process(&vec![b'x'; 16 * 1024]);
    }
    assert!(screen.bytes <= OUTPUT_BYTES);
    assert_eq!(
        screen
            .observe(Some(initial.revision), Some(&restore(RestoreMode::Live)))
            .unwrap()
            .frames[0]
            .0,
        Opcode::Restore
    );
}

#[test]
fn mouse_modes_gate_events_and_encode_sgr_x10_and_utf8() {
    let mut screen = screen();
    assert!(screen.mouse(0, 0, 0, MouseAction::Down).unwrap().is_empty());
    screen.process(b"\x1b[?1000h\x1b[?1006h");
    assert_eq!(
        screen.mouse(1, 2, 0, MouseAction::Down).unwrap(),
        b"\x1b[<0;3;2M"
    );
    assert_eq!(
        screen.mouse(1, 2, 0, MouseAction::Up).unwrap(),
        b"\x1b[<0;3;2m"
    );
    assert!(screen.mouse(0, 0, 0, MouseAction::Move).unwrap().is_empty());
    screen.process(b"\x1b[?1003h");
    assert_eq!(
        screen.mouse(1, 2, 0, MouseAction::Move).unwrap(),
        b"\x1b[<32;3;2M"
    );
    screen.process(b"\x1b[?1006l");
    assert_eq!(
        screen.mouse(1, 2, 0, MouseAction::Down).unwrap(),
        [27, 91, 77, 32, 35, 34]
    );
    screen.process(b"\x1b[?1005h");
    assert_eq!(
        screen.mouse(1, 2, 0, MouseAction::Up).unwrap(),
        [27, 91, 77, 35, 35, 34]
    );
    assert_eq!(
        screen.mouse(99, 0, 0, MouseAction::Down),
        Err(Error::Invalid)
    );
    assert_eq!(
        screen.mouse(0, 0, 99, MouseAction::Down),
        Err(Error::Invalid)
    );
}

#[test]
fn indexed_colors_alt_screen_and_wrapped_rows_are_observable() {
    let mut screen = screen();
    screen.process(b"abcdefghijklmnop\r\n\x1b[31;48;5;200mcolors");
    let snapshot = screen.snapshot(0);
    assert_eq!(snapshot["gridWrapped"][0], true);
    assert_eq!(snapshot["grid"][2][0]["fgMode"], 1);
    assert_eq!(snapshot["grid"][2][0]["bgMode"], 2);
    screen.process(b"\x1b[?1049h\x1b[Halt");
    assert!(screen.capture()[0].starts_with("alt"));
    screen.process(b"\x1b[?1049l");
    assert!(screen.capture().join("\n").contains("colors"));
}

#[test]
fn resize_preserves_partial_utf8_and_alternate_screen_restore_preserves_main_buffer() {
    let mut screen = screen();
    let bytes = "你".as_bytes();
    screen.process(&bytes[..1]);
    screen.resize(Size { rows: 4, cols: 15 });
    screen.process(&bytes[1..]);
    assert_eq!(screen.capture()[0], "你");
    screen.process(b" main\x1b[?1049h\x1b[Halternate");
    let ansi = screen.ansi(10);
    let mut restored = Screen::new(Size { rows: 4, cols: 15 });
    restored.process(&ansi);
    assert!(restored.parser.screen().alternate_screen());
    assert_eq!(restored.capture(), screen.capture());
    restored.process(b"\x1b[?1049l");
    screen.process(b"\x1b[?1049l");
    assert_eq!(restored.capture(), screen.capture());
}

#[test]
fn styled_wrapped_history_restores_without_inserting_or_losing_cells() {
    let mut screen = screen();
    screen.process(
        b"\x1b[31mabcdefghijklmnopqrstuvwxyz\x1b[0m\r\nsecond long line of output\r\nthird\r\nlast",
    );
    let mut restored = Screen::new(Size { rows: 3, cols: 12 });
    restored.process(&screen.ansi(500));
    assert_eq!(restored.capture(), screen.capture());
    assert_eq!(restored.snapshot(0)["grid"], screen.snapshot(0)["grid"]);
}

#[test]
fn wide_desktop_restores_stay_bounded_and_oversized_legacy_snapshots_fail_locally() {
    let size = Size {
        rows: 100,
        cols: 500,
    }
    .validate()
    .unwrap();
    let mut screen = Screen::new(size);
    let line = format!(
        "\x1b[1;2;3;4;7;38;2;12;34;56;48;2;65;43;21m{}",
        "x".repeat(500)
    );
    for row in 1..=100 {
        screen.process(format!("\x1b[{row};1H{line}").as_bytes());
    }
    assert!(matches!(screen.observe(None, None), Err(Error::Exhausted)));
    let restored = screen
        .observe(None, Some(&restore(RestoreMode::VisibleSnapshot)))
        .unwrap();
    assert_eq!(restored.frames[0].0, Opcode::Restore);
    assert!(restored.frames[0].1.len() < model::server::MAX_QUEUE_BYTES / 2);
}
