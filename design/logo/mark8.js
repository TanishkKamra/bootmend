// Round 8: heavy, architectural marks built from joinery. 48-unit grid.
// Each build(id) returns { defs, a, b }: a = main mass, b = the key piece (the mend).
const f2 = (p) => p.map((v) => v.toFixed(2)).join(' ');
const D = (d) => d * Math.PI / 180;
function roundPoly(pts, r) {
  const n = pts.length; let d = '';
  for (let i = 0; i < n; i++) {
    const p0 = pts[(i + n - 1) % n], p1 = pts[i], p2 = pts[(i + 1) % n];
    const v1 = [p0[0] - p1[0], p0[1] - p1[1]], v2 = [p2[0] - p1[0], p2[1] - p1[1]];
    const l1 = Math.hypot(...v1), l2 = Math.hypot(...v2), rr = Math.min(r, l1 / 2.2, l2 / 2.2);
    const a = [p1[0] + v1[0] / l1 * rr, p1[1] + v1[1] / l1 * rr], b = [p1[0] + v2[0] / l2 * rr, p1[1] + v2[1] / l2 * rr];
    d += (i ? 'L' : 'M') + f2(a) + 'Q' + f2(p1) + ' ' + f2(b);
  }
  return d + 'Z';
}
// Rotate a set of polygons, then scale and centre them in a box (w x h) around (24, cy).
function place(polys, deg, w, h, cy = 24) {
  const c = Math.cos(D(deg)), s = Math.sin(D(deg));
  const rot = polys.map((p) => p.map(([x, y]) => [x * c - y * s, x * s + y * c]));
  const all = rot.flat(), xs = all.map((p) => p[0]), ys = all.map((p) => p[1]);
  const [x0, x1, y0, y1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
  const k = Math.min(w / (x1 - x0), h / (y1 - y0)), mx = (x0 + x1) / 2, my = (y0 + y1) / 2;
  return { polys: rot.map((p) => p.map(([x, y]) => [24 + (x - mx) * k, cy + (y - my) * k])), k };
}
function sectorPath(cx, cy, r1, r2, a1, a2) {
  const P = (r, a) => [cx + r * Math.cos(D(a)), cy + r * Math.sin(D(a))];
  const L = a2 - a1 > 180 ? 1 : 0;
  return `M${f2(P(r2, a1))}A${r2} ${r2} 0 ${L} 1 ${f2(P(r2, a2))}L${f2(P(r1, a2))}A${r1} ${r1} 0 ${L} 0 ${f2(P(r1, a1))}Z`;
}
const bowtie = (cx, cy, w, h, waist) => [[cx - w / 2, cy - h / 2], [cx, cy - waist / 2], [cx + w / 2, cy - h / 2], [cx + w / 2, cy + h / 2], [cx, cy + waist / 2], [cx - w / 2, cy + h / 2]];

// A check (a "┘" turned 45°) in local units: t = stroke, H = long arm, S = short arm.
const KDY = { v: -2.6 }, ASX = { v: 1 }, ICONOPT = { inset: 3, scale: 0.66, shadow: true };
const T = 10.5, H = 36, S = 24, G = 1.7;
function checkMark(kind) {
  const longP = kind === 'lap'
    ? [[S - T, 0], [S, 0], [S, H], [S - T / 2, H], [S - T / 2, H - T / 2], [S - T, H - T / 2]]
    : [[S - T, 0], [S, 0], [S, H], [S - T, H]];
  const shortP = kind === 'lap'
    ? [[0, H - T], [S - T - G, H - T], [S - T - G, H - T / 2 + G], [S - T / 2 - G, H - T / 2 + G], [S - T / 2 - G, H], [0, H]]
    : [[0, H - T], [S - T - G, H - T], [S - T - G, H], [0, H]];
  const polys = [longP, shortP];
  if (kind === 'keyed') polys.push(bowtie(S - T - G / 2, H - T / 2, 13, 7.4, 3.2));
  if (kind === 'knee') {
    polys[0] = [[S - T, 0], [S, 0], [S, H - T - G], [S - T, H - T - G]];
    polys[1] = [[0, H - T], [S - T - G, H - T], [S - T - G, H], [0, H]];
    polys.push([[S - T, H - T], [S, H - T], [S, H], [S - T, H]]);
  }
  return place(polys, 45, 38, 33, 25.5);
}

const MARKS = {
  _joint: {
    name: 'Joined check', idea: 'A check-up mark built from two solid blocks, joined where they meet.',
    build() { const { polys } = checkMark('plain'); return { defs: '', a: `<path d="${roundPoly(polys[0], 1.3)}"/>`, b: `<path d="${roundPoly(polys[1], 1.3)}"/>` }; },
  },
  lap: {
    name: 'Lap-joint check', idea: 'The same check, joined with a stepped carpenter\'s joint so the pieces lock.',
    build() { const { polys } = checkMark('lap'); return { defs: '', a: `<path d="${roundPoly(polys[0], 1.1)}"/>`, b: `<path d="${roundPoly(polys[1], 1.1)}"/>` }; },
  },
  _keyed: {
    name: 'Keyed check', idea: 'The check held together by a butterfly key, the inlay woodworkers use to mend a crack.',
    build(id) {
      const { polys, k } = checkMark('keyed');
      return {
        defs: `<mask id="${id}m" maskUnits="userSpaceOnUse" x="0" y="0" width="48" height="48"><rect width="48" height="48" fill="#fff"/><path d="${roundPoly(polys[2], 0.6)}" fill="#000" stroke="#000" stroke-width="${(G * k * 1.15).toFixed(2)}" stroke-linejoin="round"/></mask>`,
        a: `<g mask="url(#${id}m)"><path d="${roundPoly(polys[0], 1.3)}"/><path d="${roundPoly(polys[1], 1.3)}" class="mid"/></g>`,
        b: `<path d="${roundPoly(polys[2], 0.6)}"/>`,
      };
    },
  },
  arch: {
    name: 'Keystone', idea: 'An arch whose last stone is just dropping in: the piece that makes the rest hold.',
    build(id) {
      const cx = 24, cy = 31.5, r1 = 7.6, r2 = 18.6, g = 1.6, top = 44;
      const P = (r, a) => [cx + r * Math.cos(D(a)), cy + r * Math.sin(D(a))];
      const joints = [216, 324].map((a) => `M${f2(P(r1 - 1, a))}L${f2(P(r2 + 1, a))}`).join('');
      const kd = (g / 2) / ((r1 + r2) / 2) * 180 / Math.PI;
      return {
        defs: `<mask id="${id}m" maskUnits="userSpaceOnUse" x="0" y="0" width="48" height="48"><rect width="48" height="48" fill="#fff"/>
          <path d="${joints}M${cx - r2 - 1} ${cy}H${cx - r1 + 0.2}M${cx + r1 - 0.2} ${cy}H${cx + r2 + 1}" stroke="#000" stroke-width="${g}"/>
          <path d="${sectorPath(cx, cy, r1 - 1, r2 + 3, 252 - kd, 288 + kd)}" fill="#000"/></mask>`,
        a: `<g transform="translate(24 0) scale(${ASX.v} 1) translate(-24 0)"><g mask="url(#${id}m)"><path d="${sectorPath(cx, cy, r1, r2, 180, 360)}"/><path d="M${cx - r2} ${cy}H${cx - r1}V${top}H${cx - r2}Z M${cx + r1} ${cy}H${cx + r2}V${top}H${cx + r1}Z"/></g></g>`,
        b: `<g transform="translate(24 0) scale(${ASX.v} 1) translate(-24 0)"><path transform="translate(0 ${KDY.v})" d="${sectorPath(cx, cy, r1 - 0.6, r2 + 1.8, 252 + kd, 288 - kd)}"/></g>`,
      };
    },
  },
  bare: {
    name: 'Keystone, reduced', idea: 'One solid arch, and the single stone that locks it, still settling.',
    build(id) {
      const cx = 24, cy = 30.5, r1 = 7.4, r2 = 18.8, g = 1.7, top = 44;
      const kd = (g / 2) / ((r1 + r2) / 2) * 180 / Math.PI;
      return {
        defs: `<mask id="${id}m" maskUnits="userSpaceOnUse" x="0" y="0" width="48" height="48"><rect width="48" height="48" fill="#fff"/>
          <path d="${sectorPath(cx, cy, r1 - 1, r2 + 3, 252 - kd, 288 + kd)}" fill="#000"/></mask>`,
        a: `<g mask="url(#${id}m)"><path d="M${cx - r2} ${top}V${cy}A${r2} ${r2} 0 0 1 ${cx + r2} ${cy}V${top}H${cx + r1}V${cy}A${r1} ${r1} 0 0 0 ${cx - r1} ${cy}V${top}Z"/></g>`,
        b: `<path transform="translate(0 ${KDY.v})" d="${sectorPath(cx, cy, r1 - 0.6, r2 + 1.8, 252 + kd, 288 - kd)}"/>`,
      };
    },
  },
  knee: {
    name: 'Keystone check', idea: 'A check whose corner stone carries both arms. Take it away and the check falls.',
    build() {
      const { polys } = checkMark('knee');
      const up = 'transform="translate(0 -1.4)"';
      return { defs: '', a: `<path d="${roundPoly(polys[0], 1.2)}"/><path d="${roundPoly(polys[1], 1.2)}"/>`, b: `<path ${up} d="${roundPoly(polys[2], 1.2)}"/>` };
    },
  },
  span: {
    name: 'Keystone span', idea: 'The arch on its own, five stones, the last one just dropping in.',
    build(id) {
      const cx = 24, cy = 35, r1 = 9.4, r2 = 21, g = 1.6;
      const P = (r, a) => [cx + r * Math.cos(D(a)), cy + r * Math.sin(D(a))];
      const joints = [216, 324].map((a) => `M${f2(P(r1 - 1, a))}L${f2(P(r2 + 1, a))}`).join('');
      const kd = (g / 2) / ((r1 + r2) / 2) * 180 / Math.PI;
      return {
        defs: `<mask id="${id}m" maskUnits="userSpaceOnUse" x="0" y="0" width="48" height="48"><rect width="48" height="48" fill="#fff"/>
          <path d="${joints}" stroke="#000" stroke-width="${g}"/><path d="${sectorPath(cx, cy, r1 - 1, r2 + 3, 252 - kd, 288 + kd)}" fill="#000"/></mask>`,
        a: `<g mask="url(#${id}m)"><path d="${sectorPath(cx, cy, r1, r2, 180, 360)}"/></g>`,
        b: `<path transform="translate(0 -2.8)" d="${sectorPath(cx, cy, r1 - 0.6, r2 + 1.8, 252 + kd, 288 - kd)}"/>`,
      };
    },
  },
  gate: {
    name: 'Keystone gate', idea: 'One solid block with an arched way through, and its keystone setting.',
    build(id) {
      const cx = 24, cy = 31, r1 = 8.2, r2 = 15, g = 1.5, kd = (g / 2) / 11.6 * 180 / Math.PI;
      const P = (r, a) => [cx + r * Math.cos(D(a)), cy + r * Math.sin(D(a))];
      const joints = [216, 324].map((a) => `M${f2(P(r1 - 1, a))}L${f2(P(r2, a))}`).join('');
      return {
        defs: `<mask id="${id}m" maskUnits="userSpaceOnUse" x="0" y="0" width="48" height="48"><rect width="48" height="48" fill="#fff"/>
          <path d="M${cx - r1} 44V${cy}A${r1} ${r1} 0 0 1 ${cx + r1} ${cy}V44Z" fill="#000"/>
          <path d="${sectorPath(cx, cy, r1 - 1, r2 + 0.75, 252 - kd, 288 + kd)}" fill="#000"/></mask>`,
        a: `<g mask="url(#${id}m)"><rect x="6" y="9" width="36" height="33" rx="6"/></g>`,
        b: `<path transform="translate(0 -3.4)" d="${sectorPath(cx, cy, r1 - 0.4, r2 + 2.4, 252 + kd, 288 - kd)}"/>`,
      };
    },
  },
};

// Finishes: 'icon' = graphite app plate; 'light' / 'dark' = unplated mark on that surface; 'sil' = one colour.
let uid = 0;
function icon(key, size, finish, accent) {
  const id = 'm' + (uid++), parts = MARKS[key].build(id);
  const acc = accent || null;
  const g = (n, a, b) => `<linearGradient id="${id}${n}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${a}"/><stop offset="1" stop-color="${b}"/></linearGradient>`;
  const defs = g('silver', '#F4F5F7', '#B4BAC4') + g('silverK', '#9AA1AD', '#6E7581') + g('graph', '#474B54', '#1D1F24') + g('graphK', '#A3A9B4', '#7C838F')
    + g('plate', '#2B2D33', '#121317') + g('steel', '#828A97', '#666D7A') + g('cob', '#5B86FF', '#3A66F2')
    + `<filter id="${id}sh2" x="-20%" y="-20%" width="140%" height="150%"><feDropShadow dx="0" dy="0.6" stdDeviation="0.6" flood-color="#000" flood-opacity=".25"/></filter>` + (acc ? g('acc', acc[0], acc[1]) : '')
    + `<filter id="${id}sh" x="-20%" y="-20%" width="140%" height="150%"><feDropShadow dx="0" dy="0.9" stdDeviation="0.9" flood-color="#000" flood-opacity=".5"/></filter>`;
  let A, B, Mid;
  if (finish === 'sil') { A = B = Mid = '#000'; }
  else if (finish === 'app') { A = `url(#${id}steel)`; Mid = '#737A87'; B = `url(#${id}cob)`; }
  else if (finish === 'light') { A = `url(#${id}graph)`; Mid = '#2E3138'; B = acc ? `url(#${id}acc)` : `url(#${id}graphK)`; }
  else { A = `url(#${id}silver)`; Mid = '#D3D7DD'; B = acc ? `url(#${id}acc)` : `url(#${id}silverK)`; }
  const body = `<g fill="${A}">${parts.a.replace(/class="mid"/g, `fill="${Mid}"`)}</g><g fill="${B}">${parts.b}</g>`;
  const inner = finish === 'icon'
    ? (() => { const o = ICONOPT, w = 48 - 2 * o.inset, rx = w / 4;
        return `<rect x="${o.inset}" y="${o.inset}" width="${w}" height="${w}" rx="${rx}" fill="url(#${id}plate)"/><rect x="${o.inset + 0.4}" y="${o.inset + 0.4}" width="${w - 0.8}" height="${w - 0.8}" rx="${rx - 0.4}" fill="none" stroke="#fff" stroke-opacity=".1" stroke-width=".8"/>
       <g transform="translate(24 24) scale(${o.scale}) translate(-24 -24)"${o.shadow ? ` filter="url(#${id}sh)"` : ''}>${body}</g>`; })()
    : finish === 'app' ? `<g transform="translate(0 -2)" filter="url(#${id}sh2)">${body}</g>` : body;
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 48 48" width="${size}" height="${size}"><defs>${defs}${parts.defs}</defs>${inner}</svg>`;
}
