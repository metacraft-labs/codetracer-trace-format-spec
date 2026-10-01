// Generators, iterators, async functions, async generators and a small
// promise-based task scheduler with bounded concurrency.
function* range(start, end, step = 1) {
  for (let i = start; i < end; i += step) yield i;
}

function* primes() {
  const found = [];
  for (let n = 2; ; n++) {
    let isPrime = true;
    for (const p of found) {
      if (p * p > n) break;
      if (n % p === 0) { isPrime = false; break; }
    }
    if (isPrime) { found.push(n); yield n; }
  }
}

function* take(it, n) {
  let i = 0;
  for (const v of it) {
    if (i++ >= n) return;
    yield v;
  }
}

function* mapIt(it, f) { for (const v of it) yield f(v); }
function* filterIt(it, f) { for (const v of it) if (f(v)) yield v; }

class Tree {
  constructor(value, left = null, right = null) { this.value = value; this.left = left; this.right = right; }
  *[Symbol.iterator]() {
    if (this.left) yield* this.left;
    yield this.value;
    if (this.right) yield* this.right;
  }
  static build(lo, hi) {
    if (lo > hi) return null;
    const mid = (lo + hi) >> 1;
    return new Tree(mid, Tree.build(lo, mid - 1), Tree.build(mid + 1, hi));
  }
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function worker(id, job) {
  await sleep(job % 3);
  let acc = 0;
  for (const p of take(primes(), 10 + (job % 15))) acc += p;
  return { id, job, acc };
}

async function* jobStream(n) {
  for (let i = 0; i < n; i++) {
    if (i % 4 === 0) await sleep(1);
    yield i;
  }
}

async function runPool(n, limit) {
  const results = [];
  const running = new Set();
  let wid = 0;
  for await (const job of jobStream(n)) {
    const p = worker(wid++ % limit, job).then((r) => { running.delete(p); results.push(r); });
    running.add(p);
    if (running.size >= limit) await Promise.race(running);
  }
  await Promise.all(running);
  return results;
}

async function main() {
  const evens = [...filterIt(range(0, 200), (x) => x % 2 === 0)];
  console.log("evens", evens.length);
  const sq = [...take(mapIt(primes(), (p) => p * p), 25)];
  console.log("prime squares", sq.slice(-3));
  const t = Tree.build(1, 127);
  let sum = 0;
  for (const v of t) sum += v;
  console.log("tree sum", sum);
  const res = await runPool(Number(process.argv[2] || 40), 4);
  console.log("pool", res.length, res.reduce((a, r) => a + r.acc, 0));
  try {
    await Promise.all([sleep(1), Promise.reject(new Error("boom"))]);
  } catch (e) {
    console.log("caught", e.message);
  }
}

main();
