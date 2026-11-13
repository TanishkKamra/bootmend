// Exports the final Bootmend mark from mark8.js: node gen.mjs
import fs from 'node:fs';
import vm from 'node:vm';
const ctx = {};
vm.createContext(ctx);
vm.runInContext(fs.readFileSync(new URL('./mark8.js', import.meta.url), 'utf8') + ';this.icon=icon;this.ASX=ASX;this.ICONOPT=ICONOPT;', ctx);
ctx.ASX.v = 1.08;
const cobalt = ['#6A92FF', '#2F5BEA'];
const small = () => { Object.assign(ctx.ICONOPT, { inset: 1.5, scale: 0.78, shadow: false }); const s = ctx.icon('arch', 256, 'icon', cobalt); Object.assign(ctx.ICONOPT, { inset: 3, scale: 0.66, shadow: true }); return s; };
const out = {
  'bootmend-icon-small.svg': small(),
  'bootmend-icon.svg': ctx.icon('arch', 256, 'icon', cobalt),
  'bootmend-mark-light.svg': ctx.icon('arch', 256, 'light', cobalt),
  'bootmend-mark-dark.svg': ctx.icon('arch', 256, 'dark', cobalt),
  'bootmend-mark-mono.svg': ctx.icon('arch', 256, 'sil'),
};
for (const [n, s] of Object.entries(out)) fs.writeFileSync(new URL('./' + n, import.meta.url), s + '\n');
console.log(Object.keys(out).join('\n'));
