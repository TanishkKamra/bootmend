//! The Keystone, drawn from code, and the two loaders made from it.
//!
//! - Stones runs while a check-up runs. The stones lift off the arch into a ring of ten seats, and the
//!   seats fill one by one as the check gets done.
//! - Healing runs while fixes are made. The stones drift apart and come home one by one as the fixes
//!   finish, and each joint lights up as it closes.
//!
//! Stones ends by bending the full ring into a tick and the tick back into the arch. Healing ends by
//! dropping the keystone home. Either way the keystone ends seated when everything worked, and raised
//! when something still needs a look.
//!
//! Drawn with tiny-skia because the stones are concave and change shape every frame, and egui only
//! fills convex shapes. All geometry is on the logo's 48-unit grid.

use super::theme;
use eframe::egui::{self, Color32, ColorImage, Rect, TextureHandle, TextureOptions};
use std::f32::consts::PI;
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;
use tiny_skia::{
    Color, FillRule, GradientStop, LineCap, LineJoin, LinearGradient, Mask, Paint, Path, PathBuilder, Pixmap, Point, Shader, SpreadMode, Stroke, Transform,
};

type P = [f32; 2];
type Rgb = [f32; 3];

const CX: f32 = 24.0;
const CY: f32 = 31.5;
const R1: f32 = 7.6;
const R2: f32 = 18.6;
const RM: f32 = 13.1;
const FOOT: f32 = 44.0;
const GAP: f32 = 1.6;
/// The logo is drawn 8% wider than its grid.
const ASX: f32 = 1.08;
/// How far the keystone sits above its seat while something still needs a look.
pub const RAISED: f32 = 2.6;
/// The keystone reaches a little past the arch on both edges.
const KR1: f32 = 7.0;
const KR2: f32 = 20.4;
/// Where the shading of the arch starts when it is shaded as one piece.
const TOP: f32 = 12.9;

/// The ring of ten seats that Stones fills.
const RING_CX: f32 = 24.0;
const RING_CY: f32 = 24.5;
const RING_R1: f32 = 8.2;
const RING_R2: f32 = 17.2;
const TICK_W: f32 = 6.0;
/// Points along a bendable line.
const WN: usize = 160;

/// The pieces of the arch, in drawing order.
const L: usize = 0;
const V1: usize = 1;
const V2: usize = 2;
const V3: usize = 3;
const V4: usize = 4;
const R: usize = 5;
const K: usize = 6;

// Stones' ending: a beat on the full ring, the bend into a tick, a hold, the bend into the arch, and the stones setting.
const BEAT: f32 = 260.0;
const TICK: f32 = 620.0;
const HOLD: f32 = 420.0;
const ARCH: f32 = 720.0;
const SET: f32 = 520.0;

fn clamp01(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp2(a: P, b: P, t: f32) -> P {
    [lerp(a[0], b[0], t), lerp(a[1], b[1], t)]
}

fn dist(a: P, b: P) -> f32 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

/// How far `t` is between `a` and `b`, from 0 to 1.
fn seg(t: f32, a: f32, b: f32) -> f32 {
    clamp01((t - a) / (b - a))
}

fn ease_io(t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// A drop under gravity that lands at 78% of the way and gives a little on impact.
fn fall(t: f32) -> f32 {
    let t = clamp01(t);
    if t < 0.78 {
        let u = t / 0.78;
        u * u
    } else {
        1.0 + 0.07 * (PI * (t - 0.78) / 0.22).sin()
    }
}

/// A single swell up and back down, `dur` long, starting at `dt` = 0.
fn pulse(dt: f32, dur: f32) -> f32 {
    if (0.0..dur).contains(&dt) {
        (PI * dt / dur).sin()
    } else {
        0.0
    }
}

// ---------------------------------------------------------------------------------------------
// Colour

fn rgb(v: u32) -> Rgb {
    [(v >> 16 & 255) as f32, (v >> 8 & 255) as f32, (v & 255) as f32]
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t)]
}

fn color(c: Rgb, a: f32) -> Color {
    Color::from_rgba(c[0] / 255.0, c[1] / 255.0, c[2] / 255.0, clamp01(a)).unwrap_or(Color::TRANSPARENT)
}

/// The mark's colours for one theme. The keystone is always cobalt; the tick and the lights follow
/// the app's accent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pal {
    stone: [Rgb; 2],
    key: [Rgb; 2],
    accent: Rgb,
    track: Rgb,
    sheen: f32,
}

impl Pal {
    pub fn new(dark: bool, accent: Color32) -> Pal {
        let accent = [accent.r() as f32, accent.g() as f32, accent.b() as f32];
        let key = [rgb(0x6A92FF), rgb(0x2F5BEA)];
        if dark {
            Pal { stone: [rgb(0xF4F5F7), rgb(0xB4BAC4)], key, accent, track: rgb(0x3A3F48), sheen: 0.75 }
        } else {
            Pal { stone: [rgb(0x474B54), rgb(0x1D1F24)], key, accent, track: rgb(0xD5DAE2), sheen: 0.4 }
        }
    }

    fn of(ctx: &egui::Context) -> (Pal, (bool, Color32)) {
        let dark = ctx.global_style().visuals.dark_mode;
        let accent = theme::pal(ctx).accent;
        (Pal::new(dark, accent), (dark, accent))
    }
}

// ---------------------------------------------------------------------------------------------
// Geometry

fn pol(cx: f32, cy: f32, r: f32, deg: f32) -> P {
    let (s, c) = deg.to_radians().sin_cos();
    [cx + r * c, cy + r * s]
}

/// Every joint is cut parallel to its radius as the eye sees it, after the arch is stretched
/// sideways by `sx`, so each gap is exactly `g` wide from end to end, the keystone's included. The
/// face sits g/2 off the radial line; at radius `r` that is this many degrees in the unstretched circle.
fn joint_off(theta: f32, r: f32, g: f32, sx: f32) -> f32 {
    let (s, c) = theta.to_radians().sin_cos();
    let l = (sx * c).hypot(s);
    clamp01(g * l / (2.0 * r * sx)).asin().to_degrees()
}

fn arc_pts(cx: f32, cy: f32, r: f32, a0: f32, a1: f32, out: &mut Vec<P>) {
    let n = ((a1 - a0).abs() / 3.0).ceil().max(2.0) as usize;
    out.extend((0..=n).map(|i| pol(cx, cy, r, a0 + (a1 - a0) * i as f32 / n as f32)));
}

/// A stone between the joints at `lo` and `hi` degrees, cut for drawing at a sideways stretch of
/// `sx`. `f` below 1 cuts it short, for a seat that is part filled.
#[allow(clippy::too_many_arguments)]
fn wedge(cx: f32, cy: f32, r1: f32, r2: f32, lo: f32, hi: f32, g: f32, f: f32, sx: f32) -> Vec<P> {
    let span = |r: f32| {
        let a0 = lo + joint_off(lo, r, g, sx);
        let a1 = hi - joint_off(hi, r, g, sx);
        (a0, a0 + (a1 - a0) * f)
    };
    let ((o0, o1), (i0, i1)) = (span(r2), span(r1));
    let mut pts = Vec::with_capacity(32);
    arc_pts(cx, cy, r2, o0, o1, &mut pts);
    arc_pts(cx, cy, r1, i1, i0, &mut pts);
    pts
}

struct Piece {
    pts: Vec<P>,
    /// Where the stone's own shading runs from top to bottom.
    y: (f32, f32),
}

/// The seven pieces: legs, four voussoirs and the keystone. `g` is the joint width and `kg` how far
/// the keystone has grown from a plain voussoir to its full size.
fn pieces(g: f32, kg: f32, sx: f32) -> [Piece; 7] {
    let h = g / 2.0;
    let (kr1, kr2) = (lerp(R1, KR1, kg), lerp(R2, KR2, kg));
    let v = |lo: f32| Piece { pts: wedge(CX, CY, R1, R2, lo, lo + 36.0, g, 1.0, sx), y: (CY - R2, CY) };
    [
        Piece { pts: vec![[CX - R2, CY + h], [CX - R1, CY + h], [CX - R1, FOOT], [CX - R2, FOOT]], y: (CY, FOOT) },
        v(180.0),
        v(216.0),
        v(288.0),
        v(324.0),
        Piece { pts: vec![[CX + R1, CY + h], [CX + R2, CY + h], [CX + R2, FOOT], [CX + R1, FOOT]], y: (CY, FOOT) },
        Piece { pts: wedge(CX, CY, kr1, kr2, 252.0, 288.0, g, 1.0, sx), y: (0.0, 0.0) },
    ]
}

/// The gap of the joint at `ang` degrees, from the arch's inner edge to its outer edge: the shape
/// light shows through.
fn joint_gap(ang: f32) -> Vec<P> {
    let (o2, o1) = (joint_off(ang, R2, GAP, ASX), joint_off(ang, R1, GAP, ASX));
    vec![pol(CX, CY, R2, ang - o2), pol(CX, CY, R2, ang + o2), pol(CX, CY, R1, ang + o1), pol(CX, CY, R1, ang - o1)]
}

fn bbox_y(pts: &[P]) -> (f32, f32) {
    pts.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p[1]), b.max(p[1])))
}

fn centroid(pts: &[P]) -> P {
    let n = pts.len() as f32;
    let (x, y) = pts.iter().fold((0.0, 0.0), |(x, y), p| (x + p[0], y + p[1]));
    [x / n, y / n]
}

/// Where a piece is pushed, turned and scaled about its centre, and how opaque it is.
#[derive(Clone, Copy, Debug)]
struct Tr {
    dx: f32,
    dy: f32,
    rot: f32,
    s: f32,
    a: f32,
}

impl Default for Tr {
    fn default() -> Self {
        Tr { dx: 0.0, dy: 0.0, rot: 0.0, s: 1.0, a: 1.0 }
    }
}

fn moved(pts: &[P], tr: &Tr) -> Vec<P> {
    if tr.dx == 0.0 && tr.dy == 0.0 && tr.rot == 0.0 && tr.s == 1.0 {
        return pts.to_vec();
    }
    let [cx, cy] = centroid(pts);
    let (s, c) = tr.rot.to_radians().sin_cos();
    pts.iter()
        .map(|p| {
            let (u, v) = ((p[0] - cx) * tr.s, (p[1] - cy) * tr.s);
            [cx + u * c - v * s + tr.dx, cy + u * s + v * c + tr.dy]
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Drawing

/// A closed shape through `pts`, stretched sideways by `sx` about the mark's centre line.
fn poly(pts: &[P], sx: f32) -> Option<Path> {
    let mut pb = PathBuilder::new();
    for (i, p) in pts.iter().enumerate() {
        let x = CX + (p[0] - CX) * sx;
        if i == 0 {
            pb.move_to(x, p[1]);
        } else {
            pb.line_to(x, p[1]);
        }
    }
    pb.close();
    pb.finish()
}

/// Curves through the midpoints rather than straight pieces: a stroke along a long run of tiny
/// straight segments shows hairline cracks inside it.
fn line(pts: &[P], i0: usize, i1: usize, closed: bool) -> Option<Path> {
    let mid = |a: P, b: P| lerp2(a, b, 0.5);
    let mut pb = PathBuilder::new();
    if closed {
        // pts[i1] repeats pts[i0].
        let last = i1 - 1;
        let m = mid(pts[last], pts[i0]);
        pb.move_to(m[0], m[1]);
        for i in i0..=last {
            let m = mid(pts[i], pts[if i < last { i + 1 } else { i0 }]);
            pb.quad_to(pts[i][0], pts[i][1], m[0], m[1]);
        }
        pb.close();
    } else {
        pb.move_to(pts[i0][0], pts[i0][1]);
        for i in i0 + 1..i1 {
            let m = mid(pts[i], pts[i + 1]);
            pb.quad_to(pts[i][0], pts[i][1], m[0], m[1]);
        }
        pb.line_to(pts[i1][0], pts[i1][1]);
    }
    pb.finish()
}

#[derive(Clone, Copy)]
enum Ink {
    Flat(Rgb, f32),
    /// Shaded from `c[0]` at `y0` to `c[1]` at `y1`.
    Down {
        y0: f32,
        y1: f32,
        c: [Rgb; 2],
        a: f32,
    },
}

/// A pixmap with the mark's grid mapped onto it.
pub struct Canvas<'a> {
    pm: &'a mut Pixmap,
    t: Transform,
    /// Opacity applied to everything drawn, like a canvas's global alpha.
    a: f32,
    pal: Pal,
}

impl Canvas<'_> {
    fn paint(&self, ink: Ink) -> Option<Paint<'static>> {
        let shader = match ink {
            Ink::Flat(c, a) => Shader::SolidColor(color(c, a * self.a)),
            Ink::Down { y0, y1, c, a } => LinearGradient::new(
                Point::from_xy(0.0, y0),
                Point::from_xy(0.0, y1.max(y0 + 0.01)),
                vec![GradientStop::new(0.0, color(c[0], a * self.a)), GradientStop::new(1.0, color(c[1], a * self.a))],
                SpreadMode::Pad,
                Transform::identity(),
            )?,
        };
        Some(Paint { shader, anti_alias: true, ..Paint::default() })
    }

    fn fill(&mut self, pts: &[P], sx: f32, ink: Ink) {
        if let (Some(path), Some(paint)) = (poly(pts, sx), self.paint(ink)) {
            self.pm.fill_path(&path, &paint, FillRule::Winding, self.t, None);
        }
    }

    /// Draws with everything scaled by `k` about `c`.
    fn around(&mut self, k: f32, c: P, f: impl FnOnce(&mut Self)) {
        if (k - 1.0).abs() < 1e-4 {
            return f(self);
        }
        let old = self.t;
        self.t = old.pre_translate(c[0], c[1]).pre_scale(k, k).pre_translate(-c[0], -c[1]);
        f(self);
        self.t = old;
    }

    /// Draws at `a` times the current opacity.
    fn faded(&mut self, a: f32, f: impl FnOnce(&mut Self)) {
        if a <= 0.002 {
            return;
        }
        let old = self.a;
        self.a *= a.min(1.0);
        f(self);
        self.a = old;
    }
}

/// How to draw the arch. The defaults draw the logo with its keystone seated.
struct Arch {
    /// 1 leaves the joints open, 0 closes them.
    seam: f32,
    /// 1 shades each stone on its own, 0 shades the arch as one piece.
    sep: f32,
    key_grow: f32,
    key_dy: f32,
    off: [Tr; 7],
    only: Option<[bool; 7]>,
}

impl Default for Arch {
    fn default() -> Self {
        Arch { seam: 1.0, sep: 1.0, key_grow: 1.0, key_dy: 0.0, off: [Tr::default(); 7], only: None }
    }
}

fn arch(cv: &mut Canvas, o: &Arch) {
    let pal = cv.pal;
    for (i, pc) in pieces(GAP * o.seam, o.key_grow, ASX).iter().enumerate() {
        if o.only.is_some_and(|only| !only[i]) {
            continue;
        }
        let tr = o.off[i];
        if tr.a < 0.003 {
            continue;
        }
        let mut pts = moved(&pc.pts, &tr);
        let ink = if i == K {
            for p in &mut pts {
                p[1] += o.key_dy;
            }
            let (y0, y1) = bbox_y(&pts);
            Ink::Down { y0, y1, c: pal.key, a: tr.a }
        } else {
            Ink::Down { y0: lerp(TOP, pc.y.0, o.sep) + tr.dy, y1: lerp(FOOT, pc.y.1, o.sep) + tr.dy, c: pal.stone, a: tr.a }
        };
        cv.fill(&pts, ASX, ink);
    }
}

// ---------------------------------------------------------------------------------------------
// The bendable line: a shape as headings along its length, so one shape can bend into another
// without stretching or tearing.

struct Shape {
    p: Vec<P>,
    h: Vec<f32>,
    l: f32,
    c: P,
}

fn resample(pts: &[P], n: usize) -> (Vec<P>, f32) {
    let mut d = vec![0.0];
    for i in 1..pts.len() {
        d.push(d[i - 1] + dist(pts[i - 1], pts[i]));
    }
    let l = d[d.len() - 1];
    let mut out = Vec::with_capacity(n + 1);
    let mut j = 1;
    for k in 0..=n {
        let s = l * k as f32 / n as f32;
        while j < pts.len() - 1 && d[j] < s {
            j += 1;
        }
        let span = d[j] - d[j - 1];
        let t = clamp01((s - d[j - 1]) / if span == 0.0 { 1.0 } else { span });
        out.push(lerp2(pts[j - 1], pts[j], t));
    }
    (out, l)
}

/// The middle of a line, weighting every piece by its length.
fn mid_c(p: &[P]) -> P {
    let n = p.len() - 1;
    let (mut x, mut y) = (0.0, 0.0);
    for i in 0..n {
        x += p[i][0] + p[i + 1][0];
        y += p[i][1] + p[i + 1][1];
    }
    [x / (2 * n) as f32, y / (2 * n) as f32]
}

fn shape(pts: &[P], n: usize) -> Shape {
    let (p, l) = resample(pts, n);
    let mut h: Vec<f32> = Vec::with_capacity(n);
    for i in 0..n {
        let mut a = (p[i + 1][1] - p[i][1]).atan2(p[i + 1][0] - p[i][0]);
        if let Some(&prev) = h.last() {
            while a - prev > PI {
                a -= 2.0 * PI;
            }
            while a - prev < -PI {
                a += 2.0 * PI;
            }
        }
        h.push(a);
    }
    let c = mid_c(&p);
    Shape { p, h, l, c }
}

/// `b`'s headings, turned by whole turns to sit nearest `a`'s, so the bend takes the short way round.
fn turned(a: &Shape, b: &Shape) -> Vec<f32> {
    let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
    let k = ((mean(&a.h) - mean(&b.h)) / (2.0 * PI)).round();
    b.h.iter().map(|v| v + 2.0 * PI * k).collect()
}

/// `a` bent `t` of the way into `b` (whose headings are `hb`). The bend sweeps along the line from
/// its start to its end.
fn bend(a: &Shape, b: &Shape, hb: &[f32], t: f32) -> Vec<P> {
    if t <= 0.0 {
        return a.p.clone();
    }
    if t >= 1.0 {
        return b.p.clone();
    }
    let n = a.h.len();
    let g = clamp01(ease_io(t));
    let step = lerp(a.l, b.l, g) / n as f32;
    let mut out = Vec::with_capacity(n + 1);
    out.push([0.0, 0.0]);
    let (mut x, mut y) = (0.0, 0.0);
    for (i, (ah, bh)) in a.h.iter().zip(hb.iter()).enumerate() {
        let s = (i as f32 + 0.5) / n as f32;
        let ti = ease_io(clamp01(t * 1.75 - s * 0.75));
        let h = ah + (bh - ah) * ti;
        x += step * h.cos();
        y += step * h.sin();
        out.push([x, y]);
    }
    let c = mid_c(&out);
    let (tx, ty) = (lerp(a.c[0], b.c[0], g) - c[0], lerp(a.c[1], b.c[1], g) - c[1]);
    for p in &mut out {
        p[0] += tx;
        p[1] += ty;
    }
    out
}

/// The same wire with `len` units of path taken off each end.
fn trim_ends(pts: &[P], len: f32) -> Vec<P> {
    let shave = |q: Vec<P>| -> Vec<P> {
        let mut left = len;
        for i in 0..q.len().saturating_sub(2) {
            let s = dist(q[i], q[i + 1]);
            if s > left {
                let mut out = vec![lerp2(q[i], q[i + 1], left / s)];
                out.extend_from_slice(&q[i + 1..]);
                return out;
            }
            left -= s;
        }
        q[q.len() - 2..].to_vec()
    };
    let mut q = shave(pts.to_vec());
    q.reverse();
    let mut q = shave(q);
    q.reverse();
    q
}

enum WireInk {
    Flat(Rgb),
    Down { y0: f32, y1: f32, c: [Rgb; 2] },
}

/// The keystone's stretch of the wire, painted in its own colours on top.
struct KeyBand {
    i0: usize,
    i1: usize,
    a: f32,
    y0: f32,
    y1: f32,
}

struct Wire {
    w: f32,
    sx: f32,
    alpha: f32,
    ink: WireInk,
    /// 1 gives round ends, 0 square ones.
    cap: f32,
    key: Option<KeyBand>,
}

impl Wire {
    fn flat(w: f32, c: Rgb) -> Wire {
        Wire { w, sx: 1.0, alpha: 1.0, ink: WireInk::Flat(c), cap: 0.0, key: None }
    }
}

fn wire(cv: &mut Canvas, pts: &[P], o: &Wire) {
    let n = pts.len() - 1;
    let t = if o.sx == 1.0 { cv.t } else { cv.t.pre_translate(CX, 0.0).pre_scale(o.sx, 1.0).pre_translate(-CX, 0.0) };
    let ink = match o.ink {
        WireInk::Flat(c) => Ink::Flat(c, o.alpha),
        WireInk::Down { y0, y1, c } => Ink::Down { y0, y1, c, a: o.alpha },
    };
    let Some(paint) = cv.paint(ink) else { return };
    // Square ends that touch or overlap leave an anti-aliasing hairline across the band, at any
    // overlap. So as the ends close in they are drawn towards each other, and from 1.2 units apart
    // the wire is one closed loop with no ends. Ends that have slid past each other first lose the
    // overlapping part, so the loop never folds back.
    let gap = dist(pts[0], pts[n]);
    let crossed = gap < 2.4 && (pts[n][0] - pts[0][0]) * (pts[0][0] - pts[1][0]) + (pts[n][1] - pts[0][1]) * (pts[0][1] - pts[1][1]) < 0.0;
    let pull = if crossed { 1.0 } else { clamp01((2.4 - gap) / 1.2) };
    let closed = pull >= 1.0;
    let mut body = if crossed { trim_ends(pts, gap / 2.0) } else { pts.to_vec() };
    if pull > 0.0 {
        let k = body.len() - 1;
        let m = lerp2(body[0], body[k], 0.5);
        body[0] = lerp2(body[0], m, pull);
        body[k] = lerp2(body[k], m, pull);
    }
    let round = !closed && o.cap >= 0.999;
    let stroke = Stroke { width: o.w, line_cap: if round { LineCap::Round } else { LineCap::Butt }, line_join: LineJoin::Round, ..Stroke::default() };
    if let Some(path) = line(&body, 0, body.len() - 1, closed) {
        cv.pm.stroke_path(&path, &paint, &stroke, t, None);
    }
    // While an end changes between round and square it is a half-ellipse. It reaches back into
    // the line, so the two pieces overlap and no anti-aliasing seam shows where they meet.
    if !closed && !round && o.cap > 0.01 {
        let h = o.w / 2.0;
        let back = (o.w * 0.2).min(1.0);
        for (p, q) in [(pts[0], pts[1]), (pts[n], pts[n - 1])] {
            let (s, c) = (p[1] - q[1]).atan2(p[0] - q[0]).sin_cos();
            let mut end: Vec<P> = (0..=16)
                .map(|i| {
                    let th = -PI / 2.0 + PI * i as f32 / 16.0;
                    let (lx, ly) = (h * o.cap * th.cos(), h * th.sin());
                    [p[0] + lx * c - ly * s, p[1] + lx * s + ly * c]
                })
                .collect();
            end.push([p[0] - h * s - back * c, p[1] + h * c - back * s]);
            end.push([p[0] + h * s - back * c, p[1] - h * c - back * s]);
            if let Some(path) = poly(&end, 1.0) {
                cv.pm.fill_path(&path, &paint, FillRule::Winding, t, None);
            }
        }
    }
    if let Some(k) = o.key.as_ref().filter(|k| k.a > 0.003) {
        let stroke = Stroke { line_cap: LineCap::Butt, ..stroke };
        if let (Some(path), Some(paint)) = (line(pts, k.i0, k.i1, false), cv.paint(Ink::Down { y0: k.y0, y1: k.y1, c: cv.pal.key, a: o.alpha * k.a })) {
            cv.pm.stroke_path(&path, &paint, &stroke, t, None);
        }
    }
}

/// The lines the loaders bend between, built once.
struct Shapes {
    arch: Shape,
    tick: Shape,
    ring: Shape,
    /// The small tick that shows in the arch's doorway when Healing is done.
    door: Shape,
    ring_to_tick: Vec<f32>,
    tick_to_arch: Vec<f32>,
    /// Where the keystone's stretch of the arch line starts and ends.
    key: (usize, usize),
    /// Each seat's shading, top to bottom.
    seat_y: [(f32, f32); 10],
}

const SEATS: [f32; 10] = [288.0, 324.0, 360.0, 396.0, 432.0, 468.0, 504.0, 540.0, 576.0, 612.0];

fn shapes() -> &'static Shapes {
    static S: OnceLock<Shapes> = OnceLock::new();
    S.get_or_init(|| {
        let mut a = vec![[CX - RM, FOOT], [CX - RM, CY]];
        arc_pts(CX, CY, RM, 180.0, 360.0, &mut a);
        a.push([CX + RM, FOOT]);
        let arch = shape(&a, WN);
        let tick = shape(&[[13.75, 28.5], [20.25, 35.0], [34.25, 21.0]], WN);
        let mut r = vec![];
        arc_pts(RING_CX, RING_CY, (RING_R1 + RING_R2) / 2.0, 270.0, -90.0, &mut r);
        let ring = shape(&r, WN);
        let door = shape(&[[19.4, 35.6], [22.6, 38.8], [28.8, 32.6]], 60);
        let ring_to_tick = turned(&ring, &tick);
        let tick_to_arch = turned(&tick, &arch);
        let leg = FOOT - CY;
        let at = |deg: f32| ((leg + deg.to_radians() * RM) / arch.l * WN as f32).round() as usize;
        let key = (at(72.0), at(108.0));
        let seat_y = SEATS.map(|lo| bbox_y(&wedge(RING_CX, RING_CY, RING_R1, RING_R2, lo, lo + 36.0, GAP, 1.0, 1.0)));
        Shapes { arch, tick, ring, door, ring_to_tick, tick_to_arch, key, seat_y }
    })
}

// ---------------------------------------------------------------------------------------------
// Healing's choreography: where each piece drifts to, when in the work it comes home, which joint
// lights as it lands, when it cracks away at the start, and the phase of its gentle bob.

const DRIFT: [(f32, f32, f32); 7] =
    [(-3.2, 1.2, -7.0), (-4.2, -0.6, -11.0), (-2.6, -3.8, -8.0), (2.6, -3.8, 8.0), (4.2, -0.6, 11.0), (3.2, 1.2, 7.0), (0.0, -6.8, 0.0)];
const HOME_AT: [(f32, f32); 7] = [(0.0, 0.28), (0.22, 0.52), (0.46, 0.78), (0.46, 0.78), (0.22, 0.52), (0.0, 0.28), (0.72, 1.0)];
const LIGHTS: [Option<f32>; 7] = [None, Some(180.0), Some(216.0), Some(324.0), Some(360.0), None, None];
const CRACK: [f32; 7] = [0.24, 0.16, 0.08, 0.08, 0.16, 0.24, 0.0];
const BOB: [f32; 7] = [0.0, 1.1, 4.2, 5.4, 3.3, 2.1, 0.6];
/// How long a joint's light lasts.
const LIGHT_MS: f32 = 560.0;
/// The keystone's fall into its seat when Healing finishes.
const DROP: f32 = 460.0;

// ---------------------------------------------------------------------------------------------
// The loader

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Stones,
    Healing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Rest,
    Open,
    Work,
    Close,
}

/// What happened during one step.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Events {
    /// The tick has just shown: the moment to say the work is done.
    pub ticked: bool,
    /// The mark is back to the logo.
    pub rested: bool,
}

pub struct Mark {
    kind: Kind,
    phase: Phase,
    /// Time in the current phase, and since the mark was made, in ms.
    t: f32,
    clock: f32,
    /// How much of the work is shown done, and how much is really done.
    p: f32,
    target: f32,
    fin: bool,
    /// Whether everything worked; the keystone seats only then.
    all: bool,
    lift: f32,
    lift_from: f32,
    lift_to: f32,
    lift_t: f32,
    /// The keystone's lift when the loader opened.
    lift0: f32,
    done_at: [f32; 10],
    /// Joints lit as they close: when, and at which angle.
    flash: Vec<(f32, f32)>,
    locked: bool,
    tick_fired: bool,
    /// Windows' animation effects are off: fade instead of moving.
    pub reduced: bool,
}

impl Mark {
    pub fn new(kind: Kind, lift: f32) -> Mark {
        Mark {
            kind,
            phase: Phase::Rest,
            t: 0.0,
            clock: 0.0,
            p: 0.0,
            target: 0.0,
            fin: false,
            all: true,
            lift,
            lift_from: lift,
            lift_to: lift,
            lift_t: 1.0,
            lift0: lift,
            done_at: [-1e9; 10],
            flash: vec![],
            locked: false,
            tick_fired: false,
            reduced: false,
        }
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// Raises (1) or seats (0) the keystone, now or over a short drop.
    pub fn set_lift(&mut self, v: f32, now: bool) {
        if now {
            (self.lift, self.lift_from, self.lift_to, self.lift_t) = (v, v, v, 1.0);
        } else if v != self.lift_to {
            (self.lift_from, self.lift_to, self.lift_t) = (self.lift, v, 0.0);
        }
    }

    /// The keystone's lift now: 1 raised, 0 seated.
    pub fn lift(&self) -> f32 {
        self.lift
    }

    pub fn start(&mut self) {
        self.phase = Phase::Open;
        (self.t, self.p, self.target, self.fin) = (0.0, 0.0, 0.0, false);
        self.lift0 = self.lift;
        self.done_at = [-1e9; 10];
        self.flash.clear();
        self.locked = false;
        self.tick_fired = false;
    }

    /// How much of the work is done, 0 to 1. It never goes back, and never shows done before
    /// `finish`.
    pub fn set(&mut self, p: f32) {
        self.target = self.target.max(p.clamp(0.0, 0.995));
    }

    /// The work is over. `all` says whether everything worked.
    pub fn finish(&mut self, all: bool) {
        self.fin = true;
        self.all = all;
        self.target = 1.0;
    }

    /// Whether the mark is still moving and needs another frame.
    pub fn moving(&self) -> bool {
        self.phase != Phase::Rest || self.lift_t < 1.0
    }

    pub fn running(&self) -> bool {
        self.phase != Phase::Rest
    }

    fn open_ms(&self) -> f32 {
        match (self.reduced, self.kind) {
            (true, _) => 240.0,
            (false, Kind::Healing) => 800.0,
            (false, Kind::Stones) => 900.0,
        }
    }

    fn close_ms(&self) -> f32 {
        match (self.kind, self.reduced) {
            (Kind::Healing, true) => 1500.0,
            (Kind::Healing, false) => 2000.0,
            (Kind::Stones, true) => 1100.0,
            (Kind::Stones, false) => BEAT + TICK + HOLD + ARCH + SET,
        }
    }

    fn tick_at(&self) -> f32 {
        match (self.kind, self.reduced) {
            (Kind::Healing, true) => 300.0,
            (Kind::Healing, false) => 700.0,
            (Kind::Stones, true) => 220.0,
            (Kind::Stones, false) => BEAT + TICK * 0.7,
        }
    }

    pub fn step(&mut self, dt: f32) -> Events {
        let mut ev = Events::default();
        self.clock += dt;
        self.t += dt;
        if self.lift_t < 1.0 {
            // With animation effects off the keystone fades between its places instead, and sooner.
            self.lift_t = (self.lift_t + dt / if self.reduced { 200.0 } else { 420.0 }).min(1.0);
            let e = if self.lift_to < self.lift_from { fall(self.lift_t) } else { ease_out(self.lift_t) };
            self.lift = if self.lift_t >= 1.0 { self.lift_to } else { lerp(self.lift_from, self.lift_to, e) };
        }
        if self.phase == Phase::Open && self.t >= self.open_ms() {
            self.phase = Phase::Work;
            self.t = 0.0;
        }
        if self.phase == Phase::Work {
            let prev = self.p;
            self.p += (self.target - self.p) * (1.0 - (-dt / 260.0).exp());
            if self.fin && self.target - self.p < 0.004 {
                self.p = 1.0;
            }
            self.track(prev, self.p);
            if self.fin && self.p >= 1.0 {
                self.phase = Phase::Close;
                self.t = 0.0;
            }
        }
        if self.phase == Phase::Close {
            if self.kind == Kind::Healing && self.all && !self.reduced && !self.locked && self.t >= DROP * 0.78 {
                self.locked = true;
                self.flash.extend([(self.clock, 252.0), (self.clock, 288.0)]);
            }
            if !self.tick_fired && self.t >= self.tick_at() {
                self.tick_fired = true;
                ev.ticked = true;
            }
            if self.t >= self.close_ms() {
                self.phase = Phase::Rest;
                self.t = 0.0;
                self.set_lift(if self.all { 0.0 } else { 1.0 }, true);
                ev.rested = true;
            }
        }
        self.flash.retain(|&(t, _)| self.clock - t <= LIGHT_MS);
        ev
    }

    fn track(&mut self, a: f32, b: f32) {
        match self.kind {
            Kind::Stones => {
                for i in 0..10 {
                    if a * 10.0 < i as f32 + 1.0 && b * 10.0 >= i as f32 + 0.999 {
                        self.done_at[i] = self.clock;
                    }
                }
            }
            Kind::Healing => {
                for (i, &(w0, w1)) in HOME_AT.iter().enumerate() {
                    if seg(a, w0, w1) < 1.0 && seg(b, w0, w1) >= 1.0 {
                        if let Some(ang) = LIGHTS[i] {
                            self.flash.push((self.clock, ang));
                        }
                    }
                }
            }
        }
    }

    pub fn draw(&self, cv: &mut Canvas) {
        match self.phase {
            Phase::Rest if self.reduced && self.lift_t < 1.0 => {
                with_keys(cv, &[(RAISED * self.lift_from, 1.0 - self.lift_t), (RAISED * self.lift_to, self.lift_t)]);
            }
            Phase::Rest => arch(cv, &Arch { key_dy: -RAISED * self.lift, ..Arch::default() }),
            Phase::Open => {
                let u = clamp01(self.t / self.open_ms());
                if self.reduced {
                    cv.faded(1.0 - u, |cv| arch(cv, &Arch { key_dy: -RAISED * self.lift0, ..Arch::default() }));
                    cv.faded(u, |cv| self.work(cv, 0.0));
                } else {
                    match self.kind {
                        Kind::Stones => self.stones_open(cv, u),
                        Kind::Healing => self.heal_open(cv, u),
                    }
                }
            }
            Phase::Work => self.work(cv, self.p),
            Phase::Close => match self.kind {
                Kind::Stones if self.reduced => self.stones_close_reduced(cv, self.t),
                Kind::Stones => self.stones_close(cv, self.t),
                Kind::Healing => self.heal_close(cv, self.t),
            },
        }
    }

    fn work(&self, cv: &mut Canvas, p: f32) {
        match self.kind {
            Kind::Stones => self.stones_work(cv, p),
            Kind::Healing => self.heal_work(cv, p),
        }
    }

    /// The finished ring bends into a tick, holds, then bends into the arch and the stones set.
    fn finish_from(&self, cv: &mut Canvas, t: f32) {
        let s = shapes();
        let (t1, t2, t3) = (TICK, TICK + HOLD, TICK + HOLD + ARCH);
        let accent = cv.pal.accent;
        if t < t1 {
            let u = t / t1;
            let e = ease_io(u);
            let w = Wire { w: lerp(RING_R2 - RING_R1, TICK_W, e), cap: e, ..Wire::flat(0.0, accent) };
            wire(cv, &bend(&s.ring, &s.tick, &s.ring_to_tick, u), &w);
        } else if t < t2 {
            cv.around(1.0 + 0.05 * pulse(t - t1, 280.0), [24.0, 28.0], |cv| wire(cv, &s.tick.p, &Wire { cap: 1.0, ..Wire::flat(TICK_W, accent) }));
        } else if t < t3 {
            let u = (t - t2) / ARCH;
            let (e, c) = (ease_io(u), ease_io(seg(u, 0.3, 1.0)));
            let w = lerp(TICK_W, 11.0, e);
            let pts = bend(&s.tick, &s.arch, &s.tick_to_arch, u);
            let (ky0, ky1) = bbox_y(&pts[s.key.0..=s.key.1]);
            let stone = cv.pal.stone;
            wire(
                cv,
                &pts,
                &Wire {
                    w,
                    sx: lerp(1.0, ASX, e),
                    alpha: 1.0,
                    ink: WireInk::Down { y0: TOP, y1: FOOT, c: [mix(accent, stone[0], c), mix(accent, stone[1], c)] },
                    cap: 1.0 - e,
                    key: Some(KeyBand { i0: s.key.0, i1: s.key.1, a: c, y0: ky0 - w / 2.0, y1: ky1 + w / 2.0 }),
                },
            );
        } else {
            let u = clamp01((t - t3) / SET);
            let e = ease_io(u);
            let key_dy = if !self.all {
                -RAISED * ease_out(u)
            } else if u < 0.4 {
                -1.4 * ease_out(u / 0.4)
            } else {
                -1.4 * (1.0 - fall((u - 0.4) / 0.6))
            };
            arch(cv, &Arch { seam: e, sep: e, key_grow: ease_out(u), key_dy, ..Arch::default() });
        }
    }

    // Stones -------------------------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn seat(&self, cv: &mut Canvas, i: usize, f: f32, c: [Rgb; 2], a: f32, g: f32, sc: f32) {
        let lo = SEATS[i];
        let mut pts = wedge(RING_CX, RING_CY, RING_R1, RING_R2, lo, lo + 36.0, g, f, 1.0);
        if sc != 1.0 {
            pts = moved(&pts, &Tr { s: sc, ..Tr::default() });
        }
        let (y0, y1) = shapes().seat_y[i];
        cv.fill(&pts, 1.0, Ink::Down { y0, y1, c, a });
    }

    fn stones_open(&self, cv: &mut Canvas, u: f32) {
        let pal = cv.pal;
        let tint = ease_io(seg(u, 0.25, 0.85));
        let track = [pal.track, pal.track];
        let fade = ease_io(seg(u, 0.0, 0.45));
        let mut legs = Arch { only: Some([true, false, false, false, false, true, false]), ..Arch::default() };
        legs.off[L] = Tr { dy: 3.0 * fade, a: 1.0 - fade, ..Tr::default() };
        legs.off[R] = legs.off[L];
        arch(cv, &legs);
        for (i, d) in [(2, 0.0), (6, 0.0), (3, 1.0), (5, 1.0), (4, 2.0)] {
            let a = ease_io(seg(u, 0.35 + 0.1 * d, 0.75 + 0.1 * d));
            if a > 0.0 {
                self.seat(cv, i, 1.0, track, a, GAP, lerp(0.85, 1.0, a));
            }
        }
        // Each arch stone flies to its own seat: V3 and V4 to the first two, V1, V2 and the keystone to the last three.
        for (piece, i, order) in [(V3, 0, 0.0), (V4, 1, 1.0), (V1, 7, 1.0), (V2, 8, 0.0), (K, 9, 0.0)] {
            let lo = SEATS[i];
            let key = piece == K;
            let m = ease_io(seg(u, 0.05 * order, 0.62 + 0.05 * order));
            let cy = lerp(CY, RING_CY, m);
            let r1 = lerp(if key { KR1 } else { R1 }, RING_R1, m);
            let r2 = lerp(if key { KR2 } else { R2 }, RING_R2, m);
            let sx = lerp(ASX, 1.0, m);
            let mut pts = wedge(24.0, cy, r1, r2, lo, lo + 36.0, GAP, 1.0, sx);
            if key {
                for p in &mut pts {
                    p[1] -= RAISED * self.lift0 * (1.0 - m);
                }
            }
            let c = if key { pal.key } else { pal.stone };
            let (b0, b1) = if key {
                bbox_y(&pts)
            } else {
                let (s0, s1) = shapes().seat_y[i];
                (lerp(TOP, s0, m), lerp(CY, s1, m))
            };
            cv.fill(&pts, sx, Ink::Down { y0: b0, y1: b1, c: [mix(c[0], pal.track, tint), mix(c[1], pal.track, tint)], a: 1.0 });
        }
    }

    fn stones_work(&self, cv: &mut Canvas, p: f32) {
        let pal = cv.pal;
        for i in 0..10 {
            let f = clamp01(p * 10.0 - i as f32);
            self.seat(cv, i, 1.0, [pal.track, pal.track], 1.0, GAP, 1.0);
            if f > 0.002 {
                let sc = if self.reduced { 1.0 } else { 1.0 + 0.08 * pulse(self.clock - self.done_at[i], 280.0) };
                self.seat(cv, i, f, if i == 9 { pal.key } else { pal.stone }, 1.0, GAP, sc);
            }
        }
    }

    fn stones_close(&self, cv: &mut Canvas, t: f32) {
        if t >= BEAT {
            return self.finish_from(cv, t - BEAT);
        }
        let pal = cv.pal;
        let u = t / BEAT;
        let e = ease_io(u);
        cv.around(1.0 + 0.04 * (PI * u).sin(), [RING_CX, RING_CY], |cv| {
            wire(cv, &shapes().ring.p, &Wire { alpha: e, ..Wire::flat(RING_R2 - RING_R1, pal.accent) });
            for i in 0..10 {
                let c = if i == 9 { pal.key } else { pal.stone };
                self.seat(cv, i, 1.0, [mix(c[0], pal.accent, e), mix(c[1], pal.accent, e)], 1.0, GAP * (1.0 - e), 1.0);
            }
        });
    }

    fn stones_close_reduced(&self, cv: &mut Canvas, t: f32) {
        let accent = cv.pal.accent;
        let tick = |cv: &mut Canvas| wire(cv, &shapes().tick.p, &Wire { cap: 1.0, ..Wire::flat(TICK_W, accent) });
        let logo = |cv: &mut Canvas| arch(cv, &Arch { key_dy: if self.all { 0.0 } else { -RAISED }, ..Arch::default() });
        if t < 220.0 {
            let a = t / 220.0;
            cv.faded(1.0 - a, |cv| self.work(cv, 1.0));
            cv.faded(a, tick);
        } else if t < 880.0 {
            tick(cv);
        } else {
            let b = seg(t, 880.0, 1100.0);
            cv.faded(1.0 - b, tick);
            cv.faded(b, logo);
        }
    }

    // Healing ------------------------------------------------------------------------------------

    fn heal_open(&self, cv: &mut Canvas, u: f32) {
        let mut o = Arch::default();
        for i in 0..7 {
            let (dx, dy, rot) = DRIFT[i];
            let m = ease_out(seg(u, CRACK[i], CRACK[i] + 0.7));
            let lift = if i == K { -RAISED * self.lift0 * (1.0 - m) } else { 0.0 };
            o.off[i] = Tr { dx: dx * m, dy: dy * m + lift, rot: rot * m, s: 1.0, a: lerp(1.0, 0.38, m) };
        }
        arch(cv, &o);
    }

    fn heal_work(&self, cv: &mut Canvas, p: f32) {
        let mut o = Arch::default();
        for i in 0..7 {
            let (dx, dy, rot) = DRIFT[i];
            let h = smooth(seg(p, HOME_AT[i].0, HOME_AT[i].1));
            let r = 1.0 - h;
            let bob = if self.reduced { 0.0 } else { 0.35 * r * (self.clock * 0.0021 + BOB[i] * 1.7).sin() };
            let y = if i == K { lerp(dy, -RAISED, h) + bob } else { dy * r + bob };
            o.off[i] = Tr { dx: dx * r, dy: y, rot: rot * r, s: 1.0, a: lerp(0.38, 1.0, h) };
        }
        arch(cv, &o);
        self.lights(cv, 0.0);
    }

    /// A joint lights up as it closes: the light fills exactly the gap between the two stones.
    fn lights(&self, cv: &mut Canvas, dy: f32) {
        let accent = cv.pal.accent;
        for &(t, ang) in &self.flash {
            let age = self.clock - t;
            if !(0.0..=LIGHT_MS).contains(&age) {
                continue;
            }
            let pts: Vec<P> = joint_gap(ang).into_iter().map(|[x, y]| [x, y + dy]).collect();
            cv.fill(&pts, ASX, Ink::Flat(accent, 0.95 * (1.0 - age / LIGHT_MS)));
        }
    }

    fn heal_close(&self, cv: &mut Canvas, t: f32) {
        let red = self.reduced;
        let hit = DROP * 0.78;
        let key_dy = match (self.all, red) {
            (true, true) => 0.0,
            (true, false) => -RAISED * (1.0 - fall(t / DROP)),
            (false, true) => -RAISED,
            (false, false) => -RAISED + 0.25 * (self.clock * 0.0021).sin(),
        };
        // The landing gives the whole arch a small squash.
        let squash = if self.all && !red { 0.35 * pulse(t - hit, 240.0) } else { 0.0 };
        let mut o = Arch { key_dy: key_dy + squash, ..Arch::default() };
        for i in [L, R, V1, V2, V3, V4] {
            o.off[i].dy = squash;
        }
        arch(cv, &o);
        if self.all && !red {
            let u = seg(t, hit + 40.0, hit + 780.0);
            if u > 0.0 && u < 1.0 {
                self.sheen(cv, u);
            }
        }
        self.lights(cv, squash);
        let accent = cv.pal.accent;
        let door = |cv: &mut Canvas, pts: &[P], alpha: f32| wire(cv, pts, &Wire { cap: 1.0, alpha, ..Wire::flat(2.6, accent) });
        let s = shapes();
        if red {
            let a = seg(t, 300.0, 450.0) * (1.0 - seg(t, 1250.0, 1450.0));
            cv.faded(a, |cv| door(cv, &s.door.p, 1.0));
            return;
        }
        let drawn = ease_out(seg(t, 640.0, 960.0));
        let gone = 1.0 - seg(t, 1650.0, 1950.0);
        if drawn > 0.01 && gone > 0.0 {
            let n = ((drawn * 60.0).round() as usize).max(2);
            door(cv, &s.door.p[..=n], gone);
        }
    }

    /// A band of light passes across the stones once the keystone lands.
    fn sheen(&self, cv: &mut Canvas, u: f32) {
        let (w, h) = (cv.pm.width(), cv.pm.height());
        let Some(mut mask) = Mask::new(w, h) else { return };
        for pc in pieces(GAP, 1.0, ASX).iter() {
            if let Some(path) = poly(&pc.pts, ASX) {
                mask.fill_path(&path, FillRule::Winding, true, cv.t);
            }
        }
        let x = lerp(-8.0, 56.0, ease_io(u));
        let white = [255.0, 255.0, 255.0];
        let stops = vec![
            GradientStop::new(0.0, color(white, 0.0)),
            GradientStop::new(0.5, color(white, cv.pal.sheen * cv.a)),
            GradientStop::new(1.0, color(white, 0.0)),
        ];
        let Some(shader) = LinearGradient::new(Point::from_xy(x - 7.0, 0.0), Point::from_xy(x + 7.0, 0.0), stops, SpreadMode::Pad, Transform::identity())
        else {
            return;
        };
        let paint = Paint { shader, anti_alias: true, ..Paint::default() };
        let t = cv.t.pre_translate(24.0, 28.0).pre_rotate(-0.38_f32.to_degrees()).pre_translate(-24.0, -28.0);
        if let Some(rect) = tiny_skia::Rect::from_xywh(-30.0, -30.0, 110.0, 110.0) {
            cv.pm.fill_rect(rect, &paint, t, Some(&mask));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Onto the screen

/// Room the loaders need around the 48-unit grid, in units: the bend from the tick into the arch
/// swings past the grid's edge on its way.
const PAD: f32 = 8.0;

/// Draws into a fresh square pixmap `px` wide that shows the 48-unit grid with `pad` units of room
/// on every side.
fn render(px: u32, pad: f32, pal: Pal, draw: impl FnOnce(&mut Canvas)) -> Option<Pixmap> {
    let mut pm = Pixmap::new(px, px)?;
    let k = px as f32 / (48.0 + 2.0 * pad);
    draw(&mut Canvas { pm: &mut pm, t: Transform::from_row(k, 0.0, 0.0, k, pad * k, pad * k), a: 1.0, pal });
    Some(pm)
}

/// Shows a drawing with its 48-unit grid on `rect`, pixel for pixel, and `pad` units of room
/// around it. The texture is kept under `id` and redrawn only when `key` changes; with no key it
/// is redrawn every frame.
fn show(painter: &egui::Painter, rect: Rect, pad: f32, id: egui::Id, key: Option<u64>, draw: impl FnOnce(&mut Canvas)) {
    let ctx = painter.ctx();
    let ppp = ctx.pixels_per_point();
    let rect = rect.expand(rect.width() * pad / 48.0);
    let px = (rect.width() * ppp).round().max(1.0) as u32;
    // Whole physical pixels, so each texel lands on one screen pixel.
    let min = (rect.min.to_vec2() * ppp).round() / ppp;
    let rect = Rect::from_min_size(min.to_pos2(), egui::Vec2::splat(px as f32 / ppp));
    let (pal, theme_key) = Pal::of(ctx);
    let key = key.map(|k| {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (k, px, theme_key).hash(&mut h);
        h.finish()
    });
    let slot: Option<(TextureHandle, Option<u64>)> = ctx.data(|d| d.get_temp(id));
    let tex = match slot {
        Some((tex, Some(k))) if key == Some(k) => tex,
        slot => {
            let Some(pm) = render(px, pad, pal, draw) else { return };
            let img = ColorImage::from_rgba_premultiplied([px as usize, px as usize], pm.data());
            let tex = match slot {
                Some((mut tex, _)) => {
                    tex.set(img, TextureOptions::LINEAR);
                    tex
                }
                None => ctx.load_texture(format!("bootmend-mark-{}", id.value()), img, TextureOptions::LINEAR),
            };
            ctx.data_mut(|d| d.insert_temp(id, (tex.clone(), key)));
            tex
        }
    };
    painter.image(tex.id(), rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
}

impl Mark {
    /// Shows the mark as it is this frame.
    pub fn show(&self, painter: &egui::Painter, rect: Rect, id: egui::Id) {
        let key = (!self.moving()).then(|| (self.lift * 1000.0).round() as u64);
        show(painter, rect, PAD, id, key, |cv| self.draw(cv));
    }
}

/// The logo. Each of `keys` is a keystone to draw: how many units it sits above its seat, and its
/// opacity. A fade between two positions draws two.
pub fn logo(painter: &egui::Painter, rect: Rect, id: egui::Id, keys: &[(f32, f32)]) {
    let key = keys
        .iter()
        .fold(0_u64, |h, (lift, a)| h.wrapping_mul(1_000_003).wrapping_add(((lift * 1000.0).round() as i64 as u64) << 16 | (a * 1000.0).round() as u64));
    show(painter, rect, 0.0, id, Some(key), |cv| with_keys(cv, keys));
}

/// The arch with each of `keys` as a keystone: units above its seat, and opacity.
fn with_keys(cv: &mut Canvas, keys: &[(f32, f32)]) {
    arch(cv, &Arch { only: Some([true, true, true, true, true, true, false]), ..Arch::default() });
    for &(lift, a) in keys {
        let mut o = Arch { only: Some([false, false, false, false, false, false, true]), key_dy: -lift, ..Arch::default() };
        o.off[K].a = a;
        arch(cv, &o);
    }
}

/// The logo as it always looks, keystone raised, at `size` points. For the sidebar; it never changes.
pub fn sidebar(ui: &mut egui::Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    logo(ui.painter(), rect, egui::Id::new("bootmend-sidebar-mark"), &[(RAISED, 1.0)]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pal() -> Pal {
        Pal::new(true, Color32::from_rgb(0x8A, 0xAB, 0xFF))
    }

    fn frame(draw: impl FnOnce(&mut Canvas)) -> Pixmap {
        render(192, 0.0, pal(), draw).expect("a pixmap")
    }

    fn play(kind: Kind, lift: f32, steps: &[f32], all: bool) -> (Mark, Vec<Events>) {
        let mut m = Mark::new(kind, lift);
        m.start();
        let mut ev = vec![];
        for &p in steps {
            for _ in 0..30 {
                ev.push(m.step(16.0));
            }
            m.set(p);
        }
        m.finish(all);
        for _ in 0..600 {
            ev.push(m.step(16.0));
        }
        (m, ev)
    }

    #[test]
    fn every_joint_is_the_same_width_from_edge_to_edge_once_stretched() {
        // The distance from each face to its joint's centre line, measured where the eye sees it.
        for ang in [180.0_f32, 216.0, 252.0, 288.0, 324.0, 360.0] {
            let (s, c) = ang.to_radians().sin_cos();
            let dir = [ASX * c, s];
            let len = dir[0].hypot(dir[1]);
            for r in [R1, RM, R2] {
                for side in [-1.0, 1.0] {
                    let p = pol(CX, CY, r, ang + side * joint_off(ang, r, GAP, ASX));
                    let v = [(p[0] - CX) * ASX, p[1] - CY];
                    let off = (v[0] * dir[1] - v[1] * dir[0]).abs() / len;
                    assert!((off - GAP / 2.0).abs() < 1e-4, "joint {ang} at radius {r}: half gap {off}");
                }
            }
        }
    }

    #[test]
    fn each_loader_starts_as_the_raised_logo() {
        let raised = frame(|cv| arch(cv, &Arch { key_dy: -RAISED, ..Arch::default() }));
        for kind in [Kind::Stones, Kind::Healing] {
            let mut m = Mark::new(kind, 1.0);
            m.start();
            let first = frame(|cv| m.draw(cv));
            assert_eq!(first.data(), raised.data(), "{kind:?} jumps on its first frame");
        }
    }

    #[test]
    fn the_logo_draws_the_same_as_the_resting_loader() {
        let m = Mark::new(Kind::Stones, 1.0);
        let a = frame(|cv| m.draw(cv));
        let b = frame(|cv| {
            arch(cv, &Arch { only: Some([true, true, true, true, true, true, false]), ..Arch::default() });
            arch(cv, &Arch { only: Some([false, false, false, false, false, false, true]), key_dy: -RAISED, ..Arch::default() });
        });
        assert_eq!(a.data(), b.data());
    }

    #[test]
    fn it_ends_as_the_logo_with_the_keystone_where_the_result_says() {
        for kind in [Kind::Stones, Kind::Healing] {
            for reduced in [false, true] {
                for all in [true, false] {
                    let mut m = Mark::new(kind, 1.0);
                    m.reduced = reduced;
                    m.start();
                    m.set(0.5);
                    for _ in 0..60 {
                        m.step(16.0);
                    }
                    m.finish(all);
                    let mut ticks = 0;
                    let mut rests = 0;
                    for _ in 0..600 {
                        let e = m.step(16.0);
                        ticks += e.ticked as u32;
                        rests += e.rested as u32;
                    }
                    assert_eq!((ticks, rests), (1, 1), "{kind:?} reduced={reduced} all={all}");
                    assert!(!m.moving());
                    assert_eq!(m.lift, if all { 0.0 } else { 1.0 });
                }
            }
        }
    }

    #[test]
    fn progress_only_ever_moves_forward_and_waits_for_the_finish() {
        let mut m = Mark::new(Kind::Stones, 1.0);
        m.start();
        let mut last = 0.0;
        for p in [0.3, 0.2, 0.9, 1.0] {
            m.set(p);
            for _ in 0..200 {
                m.step(16.0);
                assert!(m.p >= last);
                last = m.p;
            }
        }
        assert!(m.p < 1.0 && m.running(), "never shows done before the work says so");
    }

    #[test]
    fn a_quick_job_still_plays_the_whole_story() {
        let (m, ev) = play(Kind::Healing, 1.0, &[], true);
        assert_eq!(ev.iter().filter(|e| e.ticked).count(), 1);
        assert!(!m.running());
    }

    #[test]
    fn healing_lights_each_joint_once_as_it_closes() {
        let (_, _) = play(Kind::Healing, 1.0, &[0.3, 0.6, 0.9], true);
        let mut m = Mark::new(Kind::Healing, 1.0);
        m.start();
        let mut seen = vec![];
        for p in [0.2, 0.4, 0.6, 0.8] {
            m.set(p);
            for _ in 0..80 {
                m.step(16.0);
                for f in &m.flash {
                    if !seen.contains(f) {
                        seen.push(*f);
                    }
                }
            }
        }
        let mut angles: Vec<i32> = seen.iter().map(|f| f.1 as i32).collect();
        angles.sort();
        assert_eq!(angles, vec![180, 216, 324, 360]);
    }

    /// The closed ring has no crack: every pixel on its middle line is solid.
    #[test]
    fn the_closed_ring_has_no_seam() {
        let mut m = Mark::new(Kind::Stones, 1.0);
        m.start();
        m.finish(true);
        while m.phase != Phase::Close {
            m.step(16.0);
        }
        // The last moment of the beat, when the ring is one closed loop.
        m.t = BEAT - 1.0;
        let pm = frame(|cv| m.draw(cv));
        let k = 192.0 / 48.0;
        for i in 0..720 {
            let [x, y] = pol(RING_CX, RING_CY, (RING_R1 + RING_R2) / 2.0, i as f32 / 2.0);
            let px = pm.pixel((x * k) as u32, (y * k) as u32).expect("inside");
            assert!(px.alpha() >= 250, "a gap in the ring at {}°", i as f32 / 2.0);
        }
    }

    /// The margin around the grid is enough: no frame of either loader reaches the texture's edge.
    #[test]
    fn nothing_is_drawn_past_the_margin() {
        let px = 80;
        for kind in [Kind::Stones, Kind::Healing] {
            for all in [true, false] {
                let mut m = Mark::new(kind, 1.0);
                m.start();
                m.set(0.6);
                m.finish(all);
                let mut n = 0;
                while !m.step(16.0).rested {
                    n += 1;
                    if n % 3 != 0 {
                        continue;
                    }
                    let pm = render(px, PAD, pal(), |cv| m.draw(cv)).expect("a pixmap");
                    let edge = (0..px).flat_map(|i| [(i, 0), (i, px - 1), (0, i), (px - 1, i)]);
                    for (x, y) in edge {
                        let a = pm.pixel(x, y).expect("inside").alpha();
                        assert_eq!(a, 0, "{kind:?} reaches the edge at ({x}, {y}) in {:?} at {} ms", m.phase, m.t);
                    }
                }
            }
        }
    }

    /// Writes contact sheets of both loaders to the folder in BOOTMEND_FRAMES, to look at by eye.
    #[test]
    #[ignore]
    fn contact_sheets() {
        let Ok(dir) = std::env::var("BOOTMEND_FRAMES") else { return };
        for dark in [false, true] {
            let pal = Pal::new(dark, if dark { Color32::from_rgb(0x8A, 0xAB, 0xFF) } else { Color32::from_rgb(0x2F, 0x5B, 0xEA) });
            for kind in [Kind::Stones, Kind::Healing] {
                for reduced in [false, true] {
                    let probe = Mark { reduced, ..Mark::new(kind, 1.0) };
                    let mut frames: Vec<(Phase, f32, f32)> = (0..10).map(|i| (Phase::Open, probe.open_ms() * (i as f32 + 0.5) / 10.0, 0.0)).collect();
                    frames.extend((0..10).map(|i| (Phase::Work, 0.0, i as f32 / 9.0)));
                    let mut t = 240.0;
                    while t <= 2540.0 {
                        frames.push((Phase::Close, t, 1.0));
                        t += 115.0;
                    }
                    let (cell, cols) = (192_u32, 10_u32);
                    let rows = (frames.len() as u32).div_ceil(cols);
                    let mut sheet = Pixmap::new(cell * cols, cell * rows).unwrap();
                    sheet.fill(if dark { Color::from_rgba8(0x1F, 0x22, 0x27, 255) } else { Color::from_rgba8(0xF6, 0xF7, 0xF9, 255) });
                    for (i, &(phase, t, p)) in frames.iter().enumerate() {
                        let mut m = Mark { reduced, ..Mark::new(kind, 1.0) };
                        m.start();
                        (m.phase, m.t, m.p, m.clock) = (phase, t, p, 5000.0);
                        if phase == Phase::Close {
                            m.fin = true;
                        }
                        let pm = render(cell, PAD, pal, |cv| m.draw(cv)).unwrap();
                        sheet.draw_pixmap(
                            ((i as u32 % cols) * cell) as i32,
                            ((i as u32 / cols) * cell) as i32,
                            pm.as_ref(),
                            &tiny_skia::PixmapPaint::default(),
                            Transform::identity(),
                            None,
                        );
                    }
                    let name = format!("{dir}/{}-{:?}{}.png", if dark { "dark" } else { "light" }, kind, if reduced { "-reduced" } else { "" });
                    sheet.save_png(&name).unwrap();
                }
            }
        }
    }

    #[test]
    fn setting_the_lift_every_frame_still_lands_in_time() {
        for (reduced, want) in [(false, 420.0), (true, 200.0)] {
            let mut m = Mark { reduced, ..Mark::new(Kind::Healing, 1.0) };
            let mut ms = 0.0;
            m.set_lift(0.0, false);
            while m.moving() && ms < 2000.0 {
                m.set_lift(0.0, false);
                m.step(16.0);
                ms += 16.0;
            }
            assert!(ms <= want + 16.0, "reduced {reduced}: took {ms} ms");
            assert_eq!(m.lift(), 0.0);
        }
    }

    #[test]
    fn with_animation_effects_off_the_keystone_fades_between_its_places() {
        let mut m = Mark { reduced: true, ..Mark::new(Kind::Healing, 1.0) };
        m.set_lift(0.0, false);
        m.step(100.0);
        let t = m.lift_t;
        assert!((t - 0.5).abs() < 1e-4);
        let got = frame(|cv| m.draw(cv));
        let want = frame(|cv| with_keys(cv, &[(RAISED, 1.0 - t), (0.0, t)]));
        assert!(got.data() == want.data(), "draws both keystones, crossfaded");
    }
}
