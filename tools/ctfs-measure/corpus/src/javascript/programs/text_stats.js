// Stdlib-heavy text processing: string methods, regexes, Map/Set,
// Array higher-order functions, JSON round-trips, sorting with comparators.
const fs = require("fs");
const path = require("path");

const BASE = [
  "The quick brown fox jumps over the lazy dog.",
  "Pack my box with five dozen liquor jugs!",
  "How vexingly quick daft zebras jump; the five boxing wizards jump quickly.",
  "Sphinx of black quartz, judge my vow. Waltz, bad nymph, for quick jigs vex.",
  "A journey of a thousand miles begins with a single step (Lao Tzu, 6th c. BC).",
  "To be, or not to be, that is the question: whether 'tis nobler in the mind to suffer.",
];

function makeCorpus(lines) {
  const out = [];
  for (let i = 0; i < lines; i++) {
    const base = BASE[i % BASE.length];
    out.push(`${i + 1}: ${i % 3 === 0 ? base.toUpperCase() : base} #tag${i % 5} user${i % 7}@example.com`);
  }
  return out.join("\n");
}

function wordFrequencies(text) {
  const freq = new Map();
  for (const word of text.toLowerCase().match(/[a-z']+/g) || []) {
    freq.set(word, (freq.get(word) || 0) + 1);
  }
  return freq;
}

function topN(freq, n) {
  return [...freq.entries()]
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
    .slice(0, n);
}

function extractEmails(text) {
  const set = new Set();
  const re = /([a-z0-9._]+)@([a-z0-9.-]+)/gi;
  let m;
  while ((m = re.exec(text)) !== null) set.add(m[0]);
  return [...set].sort();
}

function caesar(s, k) {
  return s.replace(/[a-z]/gi, (ch) => {
    const base = ch <= "Z" ? 65 : 97;
    return String.fromCharCode(((ch.charCodeAt(0) - base + k) % 26) + base);
  });
}

function wrap(text, width) {
  const words = text.split(/\s+/);
  const lines = [];
  let cur = "";
  for (const w of words) {
    if (cur.length + w.length + 1 > width && cur) {
      lines.push(cur);
      cur = w;
    } else {
      cur = cur ? cur + " " + w : w;
    }
  }
  if (cur) lines.push(cur);
  return lines;
}

function csvRoundTrip(rows) {
  const csv = rows.map((r) => r.map((c) => (/[",]/.test(c) ? `"${c.replace(/"/g, '""')}"` : c)).join(",")).join("\n");
  const parsed = csv.split("\n").map((line) => {
    const cells = [];
    let cur = "", inQ = false;
    for (let i = 0; i < line.length; i++) {
      const ch = line[i];
      if (inQ) {
        if (ch === '"' && line[i + 1] === '"') { cur += '"'; i++; }
        else if (ch === '"') inQ = false;
        else cur += ch;
      } else if (ch === '"') inQ = true;
      else if (ch === ",") { cells.push(cur); cur = ""; }
      else cur += ch;
    }
    cells.push(cur);
    return cells;
  });
  return JSON.stringify(parsed) === JSON.stringify(rows);
}

const lines = Number(process.argv[2] || 120);
const corpus = makeCorpus(lines);
const freq = wordFrequencies(corpus);
console.log("distinct words", freq.size);
console.log("top", JSON.stringify(topN(freq, 8)));
console.log("emails", extractEmails(corpus).join(" "));
const enc = caesar(BASE[0], 13);
console.log("rot13", enc, caesar(enc, 13) === BASE[0]);
console.log("wrapped", wrap(BASE.join(" "), 32).length, "lines");
const rows = corpus.split("\n").slice(0, 40).map((l) => l.split(" ").slice(0, 4));
rows.push(['quoted "x"', "a,b", "plain", ""]);
console.log("csv roundtrip", csvRoundTrip(rows));
const byTag = corpus.split("\n").reduce((acc, l) => {
  const tag = (l.match(/#(\w+)/) || [])[1];
  (acc[tag] = acc[tag] || []).push(l.length);
  return acc;
}, {});
const summary = Object.fromEntries(
  Object.entries(byTag).map(([k, v]) => [k, { n: v.length, avg: +(v.reduce((a, b) => a + b, 0) / v.length).toFixed(1) }]),
);
const tmp = path.join(require("os").tmpdir(), `ct-text-stats-${process.pid}.json`);
fs.writeFileSync(tmp, JSON.stringify(summary, null, 2));
console.log("summary", JSON.parse(fs.readFileSync(tmp, "utf8")).tag0);
fs.unlinkSync(tmp);
