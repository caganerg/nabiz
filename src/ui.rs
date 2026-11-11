//! Rendering only: turns a [`Snapshot`] into widgets. No I/O happens here.
//!
//! Layout, top to bottom: header card, cores card, memory card, disks card
//! (at most [`MAX_DISKS`] rows), then a one-line key hint pinned to the
//! bottom. Cards are as tall as their
//! content, so small machines get a compact dashboard rather than a mostly
//! empty box.

use std::collections::VecDeque;
use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Padding, Paragraph};

use crate::bar::{self, Fill, Segment};
use crate::collect::{CoreRow, Snapshot, ThreadPos};
use crate::format;
use crate::plain::ratio;
use crate::theme::{self, Theme};
use crate::topology::CoreKind;

pub const MIN_WIDTH: u16 = 50;
pub const MIN_HEIGHT: u16 = 12;
/// Narrowest a core column may get (keeps bars at least ~14 cells).
const MIN_COL_WIDTH: usize = 34;
const COL_GAP: usize = 4;
/// From this many logical CPUs on, prefer at least two columns.
const MANY_CORES: usize = 16;
/// Small machines don't need a 250-column bar; keep the dashboard compact.
const MAX_COMPACT_WIDTH: u16 = 120;
const SPARK_WIDTH: usize = 60;
const H_PAD: u16 = 2;
/// Terminals at least this tall get vertical padding inside cards.
const ROOMY_HEIGHT: u16 = 34;
/// Most disks listed; the rest are counted in the card title.
const MAX_DISKS: usize = 3;
/// Core lines kept before the disks card gets any room.
const MIN_CORE_ROWS: u16 = 3;

/// UI state that isn't measured data.
pub struct View<'a> {
    pub interval: Duration,
    /// Recent overall CPU usage, oldest first.
    pub history: &'a VecDeque<f32>,
    pub theme: Theme,
}

pub fn draw(frame: &mut Frame, snap: &Snapshot, view: &View) {
    let full = frame.area();
    if full.width < MIN_WIDTH || full.height < MIN_HEIGHT {
        draw_too_small(frame, full, &view.theme);
        return;
    }
    let width = if snap.cores.len() >= MANY_CORES {
        full.width
    } else {
        full.width.min(MAX_COMPACT_WIDTH)
    };
    let area = Rect {
        x: full.x + (full.width - width) / 2,
        width,
        ..full
    };

    let vpad = u16::from(area.height >= ROOMY_HEIGHT);
    let card_h = 4 + 2 * vpad;
    let inner_w = (width - 2 - 2 * H_PAD) as usize;
    let frame_h = 2 + 2 * vpad;
    // Rows left for core and disk lines once every other part has its share.
    let rows = area.height.saturating_sub(2 * card_h + 1 + frame_h);
    // Disks take what they need once the cores have a few lines (fewer if
    // the cores need fewer); with no room for one disk line the card goes.
    let core_min = MIN_CORE_ROWS.min(core_lines_wanted(snap, inner_w));
    let disk_rows = disk_rows_wanted(snap).min(rows.saturating_sub(core_min + frame_h) as usize);
    let disks_h = if disk_rows > 0 {
        disk_rows as u16 + frame_h
    } else {
        0
    };
    let core_rows = rows.saturating_sub(disks_h);
    let cores = core_lines(snap, &view.theme, inner_w, core_rows.max(1) as usize);
    let cores_h = cores.len() as u16 + frame_h;

    let [header, cores_area, mem, disks, _, footer] = Layout::vertical([
        Constraint::Length(card_h),
        Constraint::Length(cores_h),
        Constraint::Length(card_h),
        Constraint::Length(disks_h),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);

    let t = &view.theme;
    draw_header(frame, header, snap, view, vpad);
    let block = card(t, "Çekirdekler", vpad).title_top(cores_summary(snap, t).right_aligned());
    frame.render_widget(Paragraph::new(cores).block(block), cores_area);
    draw_memory(frame, mem, snap, t, vpad);
    if disk_rows > 0 {
        draw_disks(frame, disks, snap, t, vpad, disk_rows);
    }
    draw_footer(frame, footer, view);
}

fn card<'a>(t: &Theme, title: &'a str, vpad: u16) -> Block<'a> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(t.border())
        .title(Span::styled(format!(" {title} "), t.title()))
        .padding(Padding::new(H_PAD, H_PAD, vpad, vpad))
}

fn width(spans: &[Span]) -> usize {
    spans.iter().map(Span::width).sum()
}

/// `left` and `right` on one line, with spaces filling `w` between them.
fn spread<'a>(mut left: Vec<Span<'a>>, right: Vec<Span<'a>>, w: usize) -> Line<'a> {
    let gap = w.saturating_sub(width(&left) + width(&right)).max(1);
    left.push(Span::raw(" ".repeat(gap)));
    left.extend(right);
    Line::from(left)
}

/// Shortens `s` to at most `w` columns, ending in `…` if cut.
fn truncate(s: &str, w: usize) -> String {
    if s.chars().count() <= w {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(w.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Pads (or cuts) a plain string to exactly `w` columns, right-aligned.
fn right(s: &str, w: usize) -> String {
    format!("{:>w$}", truncate(s, w))
}

// ── Header ──────────────────────────────────────────────────────────────

fn draw_header(frame: &mut Frame, area: Rect, snap: &Snapshot, view: &View, vpad: u16) {
    let t = &view.theme;
    let w = area.width.saturating_sub(2 + 2 * H_PAD) as usize;

    let up = snap
        .uptime
        .map(|u| {
            vec![
                Span::styled("açık ", t.muted()),
                Span::raw(format::uptime(u)),
            ]
        })
        .unwrap_or_default();
    let model = snap.cpu_model.as_deref().unwrap_or("Bilinmeyen işlemci");
    let model = truncate(model, w.saturating_sub(width(&up) + 2));
    let line1 = spread(vec![Span::styled(model, t.bold())], up, w);

    let mut left = vec![Span::styled("CPU ", t.muted())];
    match snap.overall {
        Some(u) => left.push(Span::styled(
            right(&format::percent(u), 4),
            t.usage(u).patch(t.bold()),
        )),
        None if snap.measuring() => left.push(Span::styled("ölçülüyor…", t.muted())),
        None => left.push(Span::styled("   —", t.muted())),
    }
    left.push(Span::raw("    "));
    match snap.packages.as_slice() {
        [] => left.push(Span::styled("Paket —", t.muted())),
        [(_, r)] => left.extend([
            Span::styled("Paket ", t.muted()),
            Span::styled(format::celsius(Some(r.celsius)), t.heat(r).patch(t.bold())),
        ]),
        many => {
            for (id, r) in many {
                left.push(Span::styled(format!("Paket {id} "), t.muted()));
                left.push(Span::styled(format::celsius(Some(r.celsius)), t.heat(r)));
                left.push(Span::raw("  "));
            }
        }
    }
    let room = w.saturating_sub(width(&left) + 4);
    let spark = if room >= 8 {
        bar::sparkline(t, view.history.iter(), room.min(SPARK_WIDTH))
    } else {
        Vec::new()
    };
    let line2 = spread(left, spark, w);

    let block = card(t, "nabiz", vpad);
    frame.render_widget(Paragraph::new(vec![line1, line2]).block(block), area);
}

// ── Cores ───────────────────────────────────────────────────────────────

/// `6 çekirdek · 12 iş parçacığı`, or `6P + 8E çekirdek · 20 iş parçacığı`
/// on hybrids. Turkish nouns stay singular after a number.
fn cores_summary(snap: &Snapshot, t: &Theme) -> Line<'static> {
    let threads = snap.cores.len();
    let cores = if snap.hybrid {
        let p = snap.physical_cores(Some(CoreKind::Performance));
        let e = snap.physical_cores(Some(CoreKind::Efficiency));
        format!("{p}P + {e}E çekirdek")
    } else {
        let n = snap.physical_cores(None);
        format!("{n} çekirdek")
    };
    Line::styled(format!(" {cores} · {threads} iş parçacığı "), t.muted())
}

struct Group<'a> {
    heading: Option<CoreKind>,
    rows: Vec<&'a CoreRow>,
    /// Threads per physical core when uniform across the group, else 1.
    /// Columns break on multiples of this so HT pairs are never split.
    unit: usize,
}

fn groups(snap: &Snapshot) -> Vec<Group<'_>> {
    let mut out: Vec<Group> = Vec::new();
    for row in &snap.cores {
        let heading = snap.hybrid.then_some(row.kind);
        match out.last_mut() {
            Some(g) if g.heading == heading => g.rows.push(row),
            _ => out.push(Group {
                heading,
                rows: vec![row],
                unit: 1,
            }),
        }
    }
    for g in &mut out {
        g.unit = smt_unit(&g.rows);
    }
    out
}

fn smt_unit(rows: &[&CoreRow]) -> usize {
    let mut sizes: Vec<usize> = Vec::new();
    for r in rows {
        match (r.thread, sizes.last_mut()) {
            (ThreadPos::Middle | ThreadPos::Last, Some(n)) => *n += 1,
            _ => sizes.push(1),
        }
    }
    match sizes.first() {
        Some(&first) if sizes.iter().all(|&s| s == first) => first,
        _ => 1,
    }
}

/// Rows per column for `n` items in `cols` columns, rounded up so columns
/// break between physical cores.
fn rows_per_col(n: usize, cols: usize, unit: usize) -> usize {
    let rows = n.div_ceil(cols.max(1));
    if cols > 1 {
        rows.div_ceil(unit).saturating_mul(unit).min(n)
    } else {
        rows
    }
}

fn lines_needed(groups: &[Group], cols: usize) -> usize {
    let rows: usize = groups
        .iter()
        .map(|g| rows_per_col(g.rows.len(), cols, g.unit))
        .sum();
    let headings = groups.iter().filter(|g| g.heading.is_some()).count();
    rows + headings + groups.len().saturating_sub(1)
}

/// The fewest columns that fit `height`, at least two for many cores, and
/// never narrower than [`MIN_COL_WIDTH`].
fn choose_columns(groups: &[Group], total: usize, w: usize, height: usize) -> usize {
    let max_cols = ((w + COL_GAP) / (MIN_COL_WIDTH + COL_GAP)).max(1);
    let start = if total >= MANY_CORES { 2 } else { 1 }.min(max_cols);
    (start..=max_cols)
        .find(|&c| lines_needed(groups, c) <= height)
        .unwrap_or(max_cols)
}

/// Core lines needed to show every core with no height limit.
fn core_lines_wanted(snap: &Snapshot, w: usize) -> u16 {
    if snap.cores.is_empty() {
        return 1;
    }
    let groups = groups(snap);
    let cols = choose_columns(&groups, snap.cores.len(), w, usize::MAX);
    lines_needed(&groups, cols).try_into().unwrap_or(u16::MAX)
}

fn core_lines(snap: &Snapshot, t: &Theme, w: usize, height: usize) -> Vec<Line<'static>> {
    if snap.cores.is_empty() {
        return vec![Line::styled("CPU kullanımı okunamadı", t.muted())];
    }
    let groups = groups(snap);
    let cols = choose_columns(&groups, snap.cores.len(), w, height);
    let col_w = w.saturating_sub(COL_GAP * (cols - 1)) / cols;
    let label_w = snap
        .cores
        .iter()
        .map(|r| format!("cpu{}", r.cpu).len())
        .max()
        .unwrap_or(4);

    // When not everything fits, cap each group's column height to the space
    // left (keeping one line for a note), so the hidden cores are a
    // contiguous tail rather than the middle of every column.
    let overflow = lines_needed(&groups, cols) > height;
    let mut budget = if overflow {
        height.saturating_sub(1)
    } else {
        usize::MAX
    };
    let mut hidden = 0;
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (i, g) in groups.iter().enumerate() {
        let extra = usize::from(i > 0) + usize::from(g.heading.is_some());
        let rpc = rows_per_col(g.rows.len(), cols, g.unit).min(budget.saturating_sub(extra));
        if rpc == 0 {
            hidden += g.rows.len();
            continue;
        }
        budget = budget.saturating_sub(extra + rpc);
        hidden += g.rows.len().saturating_sub(rpc * cols);
        if i > 0 {
            lines.push(Line::raw(""));
        }
        if let Some(kind) = g.heading {
            lines.push(group_heading(kind, &g.rows, t));
        }
        for r in 0..rpc {
            let mut spans = Vec::new();
            for c in 0..cols {
                let Some(row) = g.rows.get(c * rpc + r) else {
                    break;
                };
                if c > 0 {
                    spans.push(Span::raw(" ".repeat(COL_GAP)));
                }
                spans.extend(core_cell(row, t, col_w, label_w));
            }
            lines.push(Line::from(spans));
        }
    }
    if hidden > 0 {
        let note = format!("+{hidden} çekirdek daha — hepsini görmek için terminali büyütün");
        lines.push(Line::styled(truncate(&note, w), t.muted()));
    }
    lines
}

fn group_heading(kind: CoreKind, rows: &[&CoreRow], t: &Theme) -> Line<'static> {
    let name = match kind {
        CoreKind::Performance => "P  Performans",
        CoreKind::Efficiency => "E  Verimlilik",
        CoreKind::Uniform => "Diğer",
    };
    let physical = rows
        .iter()
        .filter(|r| matches!(r.thread, ThreadPos::Single | ThreadPos::First))
        .count();
    Line::from(vec![
        Span::styled(name, t.kind(kind)),
        Span::styled(
            format!("   {physical} çekirdek · {} iş parçacığı", rows.len()),
            t.muted(),
        ),
    ])
}

/// `cpu0 ╭ ███████▍······  %42  51°C`, exactly `col_w` columns wide.
fn core_cell(row: &CoreRow, t: &Theme, col_w: usize, label_w: usize) -> Vec<Span<'static>> {
    // label, " ╭ ", [bar], space, pct(4), 2 spaces, temp(5)
    let fixed = label_w + 3 + 1 + 4 + 2 + 5;
    let bar_w = col_w.saturating_sub(fixed).max(1);

    let bracket = match row.thread {
        ThreadPos::Single => ' ',
        ThreadPos::First => '╭',
        ThreadPos::Middle => '├',
        ThreadPos::Last => '╰',
    };
    let mut spans = vec![
        Span::styled(
            format!("{:<label_w$}", format!("cpu{}", row.cpu)),
            t.muted(),
        ),
        Span::styled(format!(" {bracket} "), t.border()),
    ];
    let usage = row.usage.unwrap_or(0.0);
    spans.extend(bar::bar(
        t,
        bar_w,
        &[Segment::new(usage, Fill::Gradient(theme::USAGE))],
    ));
    spans.push(Span::raw(" "));
    spans.push(match row.usage {
        Some(u) => Span::styled(right(&format::percent(u), 4), t.usage(u)),
        None => Span::styled(right("…", 4), t.muted()),
    });
    spans.push(Span::raw("  "));
    let sibling = matches!(row.thread, ThreadPos::Middle | ThreadPos::Last);
    spans.push(match &row.temp {
        // Siblings repeat their core's temperature; draw the repeat dimmer.
        Some(r) => {
            let style = if sibling {
                t.heat(r).patch(t.dim())
            } else {
                t.heat(r)
            };
            Span::styled(right(&format::celsius(Some(r.celsius)), 5), style)
        }
        None => Span::styled(right("—", 5), t.muted()),
    });
    let used = fixed + bar_w;
    if used < col_w {
        spans.push(Span::raw(" ".repeat(col_w - used)));
    }
    spans
}

// ── Memory ──────────────────────────────────────────────────────────────

const MEM_LABEL_W: usize = 6;
/// Fits the widest value, `1023,9 GiB / 1023,9 GiB`.
const MEM_VALUE_W: usize = 23;

fn draw_memory(frame: &mut Frame, area: Rect, snap: &Snapshot, t: &Theme, vpad: u16) {
    let w = area.width.saturating_sub(2 + 2 * H_PAD) as usize;
    let legend = if t.mono() {
        Line::from(" █ kullanılan  ░ önbellek ")
    } else {
        Line::from(vec![
            Span::raw(" "),
            Span::styled("█", t.fg(theme::USAGE[0].1)),
            Span::styled(" kullanılan  ", t.muted()),
            Span::styled("█", t.fg(theme::CACHE)),
            Span::styled(" önbellek ", t.muted()),
        ])
    };
    let note = |s: &'static str| vec![Span::styled(s, t.muted())];
    let lines = match snap.mem {
        None => vec![
            mem_label("RAM", t, note("okunamadı")),
            mem_label("Swap", t, note("okunamadı")),
        ],
        Some(m) => vec![
            mem_row("RAM", m.used(), m.cache_shown(), m.total, t, w),
            match m.swap() {
                Some((used, total)) => mem_row("Swap", used, 0, total, t, w),
                None => mem_label("Swap", t, note("swap yok")),
            },
        ],
    };
    let block = card(t, "Bellek", vpad).title_top(legend.right_aligned());
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn mem_label(name: &str, t: &Theme, rest: Vec<Span<'static>>) -> Line<'static> {
    let mut spans = vec![Span::styled(format!("{name:<MEM_LABEL_W$}"), t.muted())];
    spans.extend(rest);
    Line::from(spans)
}

/// `RAM   ██████▊░░░░····  6,4 GiB / 15,3 GiB   %42`
fn mem_row(name: &str, used: u64, cache: u64, total: u64, t: &Theme, w: usize) -> Line<'static> {
    let frac = ratio(used, total);
    let mut segments = vec![Segment::new(frac, Fill::Gradient(theme::USAGE))];
    if cache > 0 {
        segments.push(Segment {
            frac: ratio(cache, total),
            fill: Fill::Solid(theme::CACHE),
            mono: '░',
        });
    }
    let label = Span::styled(format!("{name:<MEM_LABEL_W$}"), t.muted());
    meter_row(label, MEM_LABEL_W, &segments, frac, used, total, t, w)
}

/// `<label> <bar> <used> / <total>  <percent>`, exactly `w` columns when the
/// values fit [`MEM_VALUE_W`]. `label` must be `label_w` columns wide.
#[allow(clippy::too_many_arguments)]
fn meter_row(
    label: Span<'static>,
    label_w: usize,
    segments: &[Segment],
    frac: f32,
    used: u64,
    total: u64,
    t: &Theme,
    w: usize,
) -> Line<'static> {
    let bar_w = w.saturating_sub(label_w + 1 + MEM_VALUE_W + 2 + 4).max(1);
    let (used_s, total_s) = (format::bytes(used), format::bytes(total));
    let value_w = used_s.chars().count() + 3 + total_s.chars().count();
    let mut spans = vec![label];
    spans.extend(bar::bar(t, bar_w, segments));
    spans.push(Span::raw(
        " ".repeat(1 + MEM_VALUE_W.saturating_sub(value_w)),
    ));
    spans.push(Span::raw(used_s));
    spans.push(Span::styled(" / ", t.muted()));
    spans.push(Span::styled(total_s, t.muted()));
    spans.push(Span::raw("  "));
    spans.push(Span::styled(
        right(&format::percent(frac), 4),
        t.usage(frac),
    ));
    Line::from(spans)
}

// ── Disks ───────────────────────────────────────────────────────────────

/// Mount points longer than this are shortened with `…`.
const MAX_DISK_LABEL_W: usize = 16;

/// Lines the disks card would like: one per disk up to [`MAX_DISKS`], or
/// one for the "none" / "unreadable" note.
fn disk_rows_wanted(snap: &Snapshot) -> usize {
    snap.disks
        .as_ref()
        .map_or(1, |d| d.len().clamp(1, MAX_DISKS))
}

fn draw_disks(frame: &mut Frame, area: Rect, snap: &Snapshot, t: &Theme, vpad: u16, rows: usize) {
    let w = area.width.saturating_sub(2 + 2 * H_PAD) as usize;
    let note = |s: &'static str| vec![Line::styled(s, t.muted())];
    let (lines, hidden) = match snap.disks.as_deref() {
        None => (note("okunamadı"), 0),
        Some([]) => (note("disk bulunamadı"), 0),
        Some(disks) => {
            let shown = &disks[..rows.min(disks.len())];
            // Mount point plus a space, like `RAM   `; never so wide that the
            // bar gets shorter than 10 columns.
            let room = w.saturating_sub(1 + MEM_VALUE_W + 2 + 4 + 10);
            let label_w = shown
                .iter()
                .map(|d| d.mount.chars().count() + 1)
                .max()
                .unwrap_or(0)
                .clamp(MEM_LABEL_W, MAX_DISK_LABEL_W + 1)
                .min(room.max(MEM_LABEL_W));
            let lines = shown
                .iter()
                .map(|d| {
                    let name = truncate(&d.mount, label_w - 1);
                    let label = format!("{name:<label_w$}");
                    let frac = d.frac();
                    meter_row(
                        Span::styled(label, t.muted()),
                        label_w,
                        &[Segment::new(frac, Fill::Gradient(theme::USAGE))],
                        frac,
                        d.used,
                        d.total,
                        t,
                        w,
                    )
                })
                .collect();
            (lines, disks.len() - shown.len())
        }
    };
    let mut block = card(t, "Diskler", vpad);
    if hidden > 0 {
        block = block
            .title_top(Line::styled(format!(" +{hidden} disk daha "), t.muted()).right_aligned());
    }
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

// ── Footer & fallbacks ──────────────────────────────────────────────────

fn draw_footer(frame: &mut Frame, area: Rect, view: &View) {
    let t = &view.theme;
    let key = t.accent().patch(t.bold());
    let left = vec![
        Span::raw("  "),
        Span::styled("q", key),
        Span::styled(" çıkış    ", t.muted()),
        Span::styled("+/-", key),
        Span::styled(" aralık", t.muted()),
    ];
    let right = vec![
        Span::styled("her ", t.muted()),
        Span::styled(format::interval(view.interval), t.accent()),
        Span::raw("  "),
    ];
    frame.render_widget(
        Paragraph::new(spread(left, right, area.width as usize)),
        area,
    );
}

fn draw_too_small(frame: &mut Frame, area: Rect, t: &Theme) {
    let lines = vec![
        Line::styled("Terminal çok küçük", t.bold()),
        Line::styled(
            format!(
                "{}×{} · en az {MIN_WIDTH}×{MIN_HEIGHT} gerekli",
                area.width, area.height
            ),
            t.muted(),
        ),
    ];
    let y = area.height.saturating_sub(2) / 2;
    let centered = Rect::new(area.x, area.y + y, area.width, area.height.min(2));
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), centered);
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::collect::Collector;
    use crate::fixtures;
    use crate::sysfs::MemFs;
    use crate::theme::ColorMode;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::style::Color;

    /// Two samples from a fixture, so usage is measured.
    pub fn measured(fs: &MemFs, cpus: usize, busy: f64) -> Snapshot {
        let mut c = Collector::new(fs);
        c.collect();
        fs.set("/proc/stat", &fixtures::stat_busy(cpus, busy));
        c.collect()
    }

    fn buffer(snap: &Snapshot, w: u16, h: u16, mode: ColorMode) -> Buffer {
        let history: VecDeque<f32> = (0..80).map(|i| (i % 10) as f32 / 10.0).collect();
        let view = View {
            interval: Duration::from_secs(1),
            history: &history,
            theme: Theme::new(mode),
        };
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| draw(f, snap, &view)).unwrap();
        t.backend().buffer().clone()
    }

    fn text(buf: &Buffer) -> String {
        let area = buf.area;
        let mut out = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    pub fn render(snap: &Snapshot, w: u16, h: u16) -> String {
        text(&buffer(snap, w, h, ColorMode::TrueColor))
    }

    #[test]
    fn too_small() {
        let s = render(&Snapshot::default(), 30, 8);
        assert!(s.contains("Terminal çok küçük"), "{s}");
        // Degenerate sizes must not panic.
        for (w, h) in [(0, 0), (1, 1), (MIN_WIDTH, MIN_HEIGHT)] {
            render(&Snapshot::default(), w, h);
        }
    }

    #[test]
    fn quad_core_layout() {
        let fs = fixtures::quad_core_ht();
        let out = render(&measured(&fs, 8, 0.25), 80, 24);
        println!("{out}");
        assert!(out.contains("Intel Core i7-7700"));
        assert!(out.contains("4 çekirdek · 8 iş parçacığı"));
        assert!(out.contains("cpu0 ╭"));
        assert!(out.contains("cpu4 ╰"));
        assert!(out.contains("%25"));
        assert!(out.contains("RAM"));
        assert!(out.contains("Diskler"));
        let root = out.lines().find(|l| l.contains("│  /  ")).unwrap();
        assert!(root.contains("74,0 GiB / 221,0 GiB   %33"), "{root}");
        assert!(out.contains("/mnt/yedek"));
        assert!(!out.contains("/boot/efi"));
        assert!(out.contains("q çıkış"));
        assert!(out.contains("her 1 sn"));
    }

    #[test]
    fn hybrid_groups_and_columns() {
        let fs = fixtures::hybrid();
        let snap = measured(&fs, 20, 0.6);
        let wide = render(&snap, 120, 30);
        println!("{wide}");
        assert!(wide.contains("6P + 8E çekirdek · 20 iş parçacığı"));
        assert!(wide.contains("P  Performans"));
        assert!(wide.contains("E  Verimlilik"));
        // 20 threads ≥ 16: two columns side by side, P-cores split 6 + 6.
        let line = wide.lines().find(|l| l.contains("cpu0 ")).unwrap();
        assert!(line.contains("cpu6 "), "{line}");
    }

    #[test]
    fn ht_pairs_not_split_across_columns() {
        // 4 cores / 8 threads in a short terminal: two columns of whole pairs.
        let fs = fixtures::quad_core_ht();
        let out = render(&measured(&fs, 8, 0.5), 120, 15);
        println!("{out}");
        let first = out.lines().find(|l| l.contains("cpu0")).unwrap();
        assert!(first.contains("cpu2 ╭"), "{first}");
        assert!(!first.contains('╰'), "{first}");
    }

    #[test]
    fn many_cores_overflow_note() {
        let mut snap = measured(&fixtures::hybrid(), 20, 0.3);
        let row = snap.cores[0].clone();
        snap.cores = (0..128)
            .map(|cpu| CoreRow {
                cpu,
                thread: ThreadPos::Single,
                ..row.clone()
            })
            .collect();
        snap.hybrid = false;
        let out = render(&snap, 80, 24);
        println!("{out}");
        // The disks card takes 5 rows, leaving 8 core lines: 7 rows × 2
        // columns (cpu0–6, cpu7–13) plus the note; the rest is a contiguous
        // hidden tail.
        assert!(out.contains("+114 çekirdek daha — hepsini"), "{out}");
        let first = out.lines().find(|l| l.contains("cpu0 ")).unwrap();
        assert!(first.contains("cpu7 "), "{first}");
        assert!(!out.contains("cpu14 "));
        assert_eq!(out.lines().count(), 24);
    }

    #[test]
    fn disks_capped_and_rows_exact_width() {
        let fs = fixtures::hybrid();
        let snap = measured(&fs, 20, 0.6);
        let out = render(&snap, 100, 40);
        println!("{out}");
        // Four disks, three shown: / first, then by size.
        assert!(out.contains("+1 disk daha"), "{out}");
        assert!(out.contains("/oyunlar"));
        assert!(out.contains("/media/yedek"));
        assert!(!out.contains("/home "));
        // Disk rows end at the same column as the memory rows.
        let end = |needle: &str| {
            let l = out.lines().find(|l| l.contains(needle)).unwrap();
            l.trim_end().chars().count()
        };
        assert_eq!(end("RAM "), end("/oyunlar"));
        assert_eq!(end("Swap "), end("│  /  "));
    }

    #[test]
    fn disks_shrink_before_cores_vanish() {
        let snap = measured(&fixtures::hybrid(), 20, 0.6);
        // Header 4 + memory 4 + footer 1 + cores frame 2 leaves 6 rows at
        // height 15: three core lines, then no room for a disks card.
        let out = render(&snap, 80, 15);
        println!("{out}");
        assert!(out.contains("cpu0"), "{out}");
        assert!(!out.contains("Diskler"));
        // Two more rows: one disk line fits.
        let out = render(&snap, 80, 17);
        println!("{out}");
        assert!(
            out.contains("Diskler") && out.contains("+3 disk daha"),
            "{out}"
        );
        assert!(out.contains("cpu0"), "{out}");
        assert_eq!(out.lines().count(), 17);
        render(&snap, 80, MIN_HEIGHT);
    }

    #[test]
    fn vm_and_no_swap() {
        let vm = render(&Collector::new(&fixtures::vm()).collect(), 80, 24);
        println!("{vm}");
        assert!(vm.contains("ölçülüyor…"));
        assert!(vm.contains("Paket —"));
        assert!(vm.contains("QEMU Virtual CPU"));
        assert!(vm.contains("Diskler"));
        let ns = render(&Collector::new(&fixtures::no_swap()).collect(), 80, 24);
        assert!(ns.contains("swap yok"), "{ns}");
    }

    #[test]
    fn rows_are_exact_width() {
        // Every cell is padded so columns line up.
        let fs = fixtures::hybrid();
        let snap = measured(&fs, 20, 0.42);
        let t = Theme::new(ColorMode::TrueColor);
        for col_w in [30, 34, 51, 80] {
            let w: usize = core_cell(&snap.cores[0], &t, col_w, 5)
                .iter()
                .map(Span::width)
                .sum();
            assert_eq!(w, col_w);
        }
    }

    #[test]
    fn no_color_uses_no_colors() {
        let fs = fixtures::quad_core_ht();
        let buf = buffer(&measured(&fs, 8, 0.7), 100, 30, ColorMode::Mono);
        let out = text(&buf);
        println!("{out}");
        assert!(out.contains('█'));
        for cell in buf.content() {
            assert_eq!(cell.fg, Color::Reset, "{cell:?}");
            assert_eq!(cell.bg, Color::Reset, "{cell:?}");
        }
    }

    #[test]
    fn column_choice() {
        let snap = measured(&fixtures::hybrid(), 20, 0.1);
        let g = groups(&snap);
        assert_eq!(g.len(), 2);
        assert_eq!((g[0].unit, g[1].unit), (2, 1));
        // Wide and tall: the "many cores" minimum of two columns.
        assert_eq!(choose_columns(&g, 20, 200, 100), 2);
        // Too narrow for two columns of MIN_COL_WIDTH.
        assert_eq!(choose_columns(&g, 20, 60, 100), 1);
        // Short terminal: spread into more columns.
        assert_eq!(choose_columns(&g, 20, 200, 9), 4);
        // Nothing fits: use as many columns as the width allows.
        assert_eq!(choose_columns(&g, 20, 200, 3), 5);
        assert_eq!(rows_per_col(8, 3, 2), 4);
        assert_eq!(rows_per_col(12, 4, 2), 4);
        assert_eq!(rows_per_col(5, 1, 2), 5);
    }
}
