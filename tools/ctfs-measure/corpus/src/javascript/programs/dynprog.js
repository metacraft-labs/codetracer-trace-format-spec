// Dynamic programming: LCS, edit distance, 0/1 knapsack, coin change,
// longest increasing subsequence, memoized partition counting.
function lcs(a, b) {
  const dp = Array.from({ length: a.length + 1 }, () => new Array(b.length + 1).fill(0));
  for (let i = 1; i <= a.length; i++) {
    for (let j = 1; j <= b.length; j++) {
      dp[i][j] = a[i - 1] === b[j - 1] ? dp[i - 1][j - 1] + 1 : Math.max(dp[i - 1][j], dp[i][j - 1]);
    }
  }
  let i = a.length, j = b.length;
  const out = [];
  while (i > 0 && j > 0) {
    if (a[i - 1] === b[j - 1]) { out.push(a[i - 1]); i--; j--; }
    else if (dp[i - 1][j] >= dp[i][j - 1]) i--;
    else j--;
  }
  return out.reverse().join("");
}

function editDistance(a, b) {
  let prev = Array.from({ length: b.length + 1 }, (_, j) => j);
  for (let i = 1; i <= a.length; i++) {
    const cur = [i];
    for (let j = 1; j <= b.length; j++) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1;
      cur[j] = Math.min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + cost);
    }
    prev = cur;
  }
  return prev[b.length];
}

function knapsack(items, cap) {
  const dp = new Array(cap + 1).fill(0);
  for (const { w, v } of items) {
    for (let c = cap; c >= w; c--) {
      if (dp[c - w] + v > dp[c]) dp[c] = dp[c - w] + v;
    }
  }
  return dp[cap];
}

function coinChange(coins, amount) {
  const ways = new Array(amount + 1).fill(0);
  ways[0] = 1;
  for (const c of coins) for (let a = c; a <= amount; a++) ways[a] += ways[a - c];
  return ways[amount];
}

function lis(seq) {
  const tails = [];
  for (const x of seq) {
    let lo = 0, hi = tails.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (tails[mid] < x) lo = mid + 1; else hi = mid;
    }
    tails[lo] = x;
  }
  return tails.length;
}

const memo = new Map();
function partitions(n, k) {
  if (n === 0) return 1;
  if (n < 0 || k === 0) return 0;
  const key = n * 1000 + k;
  if (memo.has(key)) return memo.get(key);
  const r = partitions(n - k, k) + partitions(n, k - 1);
  memo.set(key, r);
  return r;
}

const scale = Number(process.argv[2] || 1);
const s1 = "ACCGGTCGAGTGCGCGGAAGCCGGCCGAA".repeat(scale);
const s2 = "GTCGTTCGGAATGCCGTTGCTCTGTAAA".repeat(scale);
console.log("lcs", lcs(s1, s2).length);
console.log("edit", editDistance("kitten sitting on the mat".repeat(scale), "sitting kitten on a hat".repeat(scale)));
const items = Array.from({ length: 20 }, (_, i) => ({ w: 3 + ((i * 7) % 11), v: 10 + ((i * 13) % 17) }));
console.log("knapsack", knapsack(items, 50 * scale));
console.log("coins", coinChange([1, 2, 5, 10, 25, 50], 100 * scale));
console.log("lis", lis(Array.from({ length: 300 * scale }, (_, i) => (i * 7919) % 1000)));
console.log("partitions", partitions(40 + 5 * scale, 40 + 5 * scale));
