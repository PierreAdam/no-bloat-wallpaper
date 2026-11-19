// Checks that every language file has exactly the same keys as English and keeps the same
// {placeholders}. Run from the repository root:  node scripts/check-translations.js
const fs = require("fs");
const path = require("path");

const ui = path.join(__dirname, "..", "gui", "ui");
global.window = {};
global.document = { documentElement: {} };
global.navigator = { language: "en" };
require(path.join(ui, "i18n.js"));
global.NBW_LANG = window.NBW_LANG;

const files = fs.readdirSync(path.join(ui, "lang")).filter((f) => f.endsWith(".js"));
for (const f of files) require(path.join(ui, "lang", f));

const placeholders = (s) => (s.match(/\{\w+\}/g) || []).sort().join(",");
const en = NBW_LANG.en;
let problems = 0;

for (const code of Object.keys(NBW_LANG).filter((c) => c !== "en")) {
  const lang = NBW_LANG[code];
  for (const key of Object.keys(en)) {
    if (!(key in lang)) { console.log(`${code}: missing "${key}"`); problems++; }
    else if (placeholders(en[key]) !== placeholders(lang[key])) { console.log(`${code}: placeholders differ in "${key}"`); problems++; }
  }
  for (const key of Object.keys(lang)) {
    if (!(key in en)) { console.log(`${code}: unknown key "${key}"`); problems++; }
  }
}

const declared = new Set(window.NBW_LANGUAGES.map(([c]) => c));
for (const code of Object.keys(NBW_LANG)) {
  if (!declared.has(code)) { console.log(`"${code}" has a file but is missing from NBW_LANGUAGES in i18n.js`); problems++; }
}

console.log(`${Object.keys(NBW_LANG).length} languages, ${Object.keys(en).length} keys, ${problems} problem(s)`);
process.exit(problems ? 1 : 0);
