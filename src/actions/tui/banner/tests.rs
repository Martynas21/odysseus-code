use super::*;

#[test]
fn wave_glyph_uses_only_swell_chars_and_ripples() {
    for x in 0..200 {
        for step in 0..50 {
            let g = wave_glyph(x, step as f64 * 0.1, WAVE_CADENCE);
            assert!(matches!(g, '^' | '~' | '_'), "unexpected wave glyph {g:?}");
        }
    }
    let a: String = (0..40).map(|x| wave_glyph(x, 0.0, WAVE_CADENCE)).collect();
    let b: String = (0..40).map(|x| wave_glyph(x, 1.5, WAVE_CADENCE)).collect();
    assert_ne!(a, b, "waterline did not move over time");
}

#[test]
fn anim_step_accelerates_smoothly_without_resetting() {
    let dt = 0.05;
    assert!(
        anim_step(0.0, dt, true) > anim_step(0.0, dt, false),
        "thinking did not speed the drift up"
    );
    let phase = 123.4;
    let next = anim_step(phase, dt, true);
    assert!(
        next > phase && next - phase < 1.0,
        "drift jumped on entering thinking: {phase} -> {next}"
    );
    assert_eq!(
        anim_step(0.0, 10.0, false),
        anim_step(0.0, 0.25, false),
        "dt was not clamped"
    );
}

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
    let hull_marks = |rows: &[String]| -> Vec<usize> {
        rows[last]
            .char_indices()
            .filter(|&(_, c)| c == '\\' || c == '/')
            .map(|(i, _)| i)
            .collect::<Vec<_>>()
    };

    let early = banner_rows(area, 0.5);
    let later = banner_rows(area, 1.5);
    assert_eq!(
        hull_marks(&early),
        hull_marks(&later),
        "boat shifted instead of staying put"
    );
    assert_ne!(
        early[last], later[last],
        "waterline did not move with the drift"
    );
}

#[test]
fn render_banner_draws_boat_on_a_rippling_waterline() {
    let area = Rect::new(0, 0, 40, BOAT_BAND_HEIGHT);
    let rows = banner_rows(area, 0.0);

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
    let phase = 20.0;
    assert!(
        scroll_x(phase, 0.0, CLOUD_SPEED, span, w) > scroll_x(phase, 1.0, CLOUD_SPEED, span, w),
        "cloud did not drift left over time"
    );
    for step in 0..200 {
        let x = scroll_x(0.0, step as f64 * 0.1, CLOUD_SPEED, span, w);
        assert!((-w..span - w).contains(&x), "cloud {x} left its range");
    }
}

#[test]
fn birds_flap_between_wing_strokes() {
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
    let sail_cells = [
        (base_x + 1, sky_top + 1),
        (base_x + 2, sky_top + 1),
        (base_x + 1, sky_top + 2),
        (base_x + 3, sky_top + 2),
    ];

    let mut a_cloud_hid_a_sail = false;
    for step in 0..400 {
        let t = step as f64 * 0.1;
        let rows = banner_rows(area, t);
        for &(x, y) in &sail_cells {
            let ch = rows[y as usize].chars().nth(x as usize).unwrap();
            assert!(
                ch != 'v' && ch != '^',
                "bird showed through the sail at ({x},{y}) t={t}"
            );
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
    put(&mut buf, bounds, 3, -1, '#', style);
    put(&mut buf, bounds, 0, 3, '#', style);
    put(&mut buf, bounds, 3, 3, '#', style);
    assert_eq!(buf[Position::new(3, 3)].symbol(), "#");
    assert_eq!(buf[Position::new(0, 3)].symbol(), " ");
}
