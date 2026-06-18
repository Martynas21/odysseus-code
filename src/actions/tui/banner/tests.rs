use super::*;

#[test]
fn wave_glyph_uses_only_swell_chars_and_ripples() {
    // The waterline is built from exactly these three glyphs.
    for x in 0..200 {
        for step in 0..50 {
            let g = wave_glyph(x, step as f64 * 0.1, WAVE_CADENCE);
            assert!(matches!(g, '^' | '~' | '_'), "unexpected wave glyph {g:?}");
        }
    }
    // It ripples on its own: the row differs as the drift advances, so the
    // water moves even while the boat sits still.
    let a: String = (0..40).map(|x| wave_glyph(x, 0.0, WAVE_CADENCE)).collect();
    let b: String = (0..40).map(|x| wave_glyph(x, 1.5, WAVE_CADENCE)).collect();
    assert_ne!(a, b, "waterline did not move over time");
}

#[test]
fn anim_step_accelerates_smoothly_without_resetting() {
    // Thinking advances the drift faster than idle over the same frame.
    let dt = 0.05;
    assert!(
        anim_step(0.0, dt, true) > anim_step(0.0, dt, false),
        "thinking did not speed the drift up"
    );
    // Crucially, flipping into thinking continues from the current phase — a
    // small forward step, never a jump — even after a long idle run. This is
    // the whole point: speed up from where we are, don't reset.
    let phase = 123.4; // a large accumulated phase, as after minutes idle
    let next = anim_step(phase, dt, true);
    assert!(
        next > phase && next - phase < 1.0,
        "drift jumped on entering thinking: {phase} -> {next}"
    );
    // A delayed frame can't lurch the scene: dt is clamped.
    assert_eq!(
        anim_step(0.0, 10.0, false),
        anim_step(0.0, 0.25, false),
        "dt was not clamped"
    );
}

/// Render the band at drift/flap time `t` into rows of plain text.
fn banner_rows(area: Rect, t: f64) -> Vec<String> {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let mut term = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
    term.draw(|f| render_banner(f, area, t, t)).unwrap();
    let buf = term.backend().buffer();
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
                .collect()
        })
        .collect()
}

#[test]
fn boat_holds_position_while_water_races() {
    let area = Rect::new(0, 0, 40, BOAT_BAND_HEIGHT);
    let last = area.height as usize - 1;
    // The hull's `\`…`/` are the only such glyphs the band ever draws, so
    // their columns pin the boat's position.
    let hull_marks = |rows: &[String]| -> Vec<usize> {
        rows[last]
            .char_indices()
            .filter(|&(_, c)| c == '\\' || c == '/')
            .map(|(i, _)| i)
            .collect::<Vec<_>>()
    };

    let early = banner_rows(area, 0.5);
    let later = banner_rows(area, 1.5);
    // As the drift advances the hull occupies the same columns — the boat
    // only ever sits at centre.
    assert_eq!(
        hull_marks(&early),
        hull_marks(&later),
        "boat shifted instead of staying put"
    );
    // But the waterline differs, because the swell has drifted on.
    assert_ne!(
        early[last], later[last],
        "waterline did not move with the drift"
    );
}

#[test]
fn render_banner_draws_boat_on_a_rippling_waterline() {
    let area = Rect::new(0, 0, 40, BOAT_BAND_HEIGHT);
    let rows = banner_rows(area, 0.0);

    // A mast stands among the sail rows, the hull rides the bottom waterline,
    // and open water (swell glyphs) flanks the hull on that same row.
    assert!(
        rows.iter().any(|r| r.contains('|')),
        "no mast/sail anywhere: {rows:?}"
    );
    let waterline = &rows[area.height as usize - 1];
    assert!(
        waterline.contains('\\') && waterline.contains('/'),
        "no hull on the waterline: {waterline:?}"
    );
    assert!(
        waterline.chars().any(|c| matches!(c, '^' | '~' | '_')),
        "no open water beside the hull: {waterline:?}"
    );
}

#[test]
fn sky_drifts_left_and_wraps() {
    let span = 45;
    let w = 5;
    // From a mid-band phase (clear of the wrap seam) the sprite slides left
    // as the drift advances.
    let phase = 20.0;
    assert!(
        scroll_x(phase, 0.0, CLOUD_SPEED, span, w) > scroll_x(phase, 1.0, CLOUD_SPEED, span, w),
        "cloud did not drift left over time"
    );
    // It never strays outside its off-screen-either-side travel range.
    for step in 0..200 {
        let x = scroll_x(0.0, step as f64 * 0.1, CLOUD_SPEED, span, w);
        assert!((-w..span - w).contains(&x), "cloud {x} left its range");
    }
}

#[test]
fn birds_flap_between_wing_strokes() {
    // Across time a bird shows both the down- and up-stroke, and nothing else.
    let seen: std::collections::HashSet<char> =
        (0..40).map(|s| bird_glyph(s as f64 * 0.1, 0.0)).collect();
    assert!(
        seen.contains(&'v') && seen.contains(&'^'),
        "bird never flapped: {seen:?}"
    );
    assert!(
        seen.iter().all(|c| matches!(c, 'v' | '^')),
        "stray glyph: {seen:?}"
    );
}

#[test]
fn sails_block_birds_but_clouds_block_sails() {
    let area = Rect::new(0, 0, 40, BOAT_BAND_HEIGHT);
    let boat_w = sprite_width(BOAT);
    let base_x = (area.width as i32 - boat_w) / 2;
    let sky_top = area.y as i32;
    // Solid sail glyphs: mast/spar on the first sail row, mast/spar on the
    // second. (Coordinates mirror the BOAT sprite.)
    let sail_cells = [
        (base_x + 1, sky_top + 1), // '|'
        (base_x + 2, sky_top + 1), // '\'
        (base_x + 1, sky_top + 2), // '|'
        (base_x + 3, sky_top + 2), // '\'
    ];

    let mut a_cloud_hid_a_sail = false;
    for step in 0..400 {
        let t = step as f64 * 0.1;
        let rows = banner_rows(area, t);
        for &(x, y) in &sail_cells {
            let ch = rows[y as usize].chars().nth(x as usize).unwrap();
            // Birds fly behind the sails, so neither wing-stroke (`v`/`^`) is
            // ever painted onto a sail.
            assert!(
                ch != 'v' && ch != '^',
                "bird showed through the sail at ({x},{y}) t={t}"
            );
            // Clouds are drawn in front, so a big one can replace a sail glyph
            // on the first sail row.
            if y == sky_top + 1 && matches!(ch, '.' | '-' | '~' | '(' | ')' | '_') {
                a_cloud_hid_a_sail = true;
            }
        }
    }
    assert!(a_cloud_hid_a_sail, "clouds never covered a sail");
}

#[test]
fn put_ignores_out_of_bounds_without_panicking() {
    use ratatui::layout::Position;
    let bounds = Rect::new(2, 2, 4, 4);
    let mut buf = Buffer::empty(Rect::new(0, 0, 10, 10));
    let style = Style::new();
    // Negative y, and coordinates outside the bounds, are all no-ops.
    put(&mut buf, bounds, 3, -1, '#', style);
    put(&mut buf, bounds, 0, 3, '#', style);
    put(&mut buf, bounds, 3, 3, '#', style);
    assert_eq!(buf[Position::new(3, 3)].symbol(), "#");
    assert_eq!(buf[Position::new(0, 3)].symbol(), " ");
}
