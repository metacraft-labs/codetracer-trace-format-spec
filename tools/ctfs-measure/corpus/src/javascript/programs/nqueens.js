// Recursion-heavy: N-queens (all solutions), permutations, Tower of Hanoi,
// Ackermann, mutual recursion, flood fill.
function queens(n) {
  const cols = new Array(n).fill(false);
  const d1 = new Array(2 * n).fill(false);
  const d2 = new Array(2 * n).fill(false);
  let count = 0;
  function place(r) {
    if (r === n) { count++; return; }
    for (let c = 0; c < n; c++) {
      if (cols[c] || d1[r + c] || d2[r - c + n]) continue;
      cols[c] = d1[r + c] = d2[r - c + n] = true;
      place(r + 1);
      cols[c] = d1[r + c] = d2[r - c + n] = false;
    }
  }
  place(0);
  return count;
}

function permutations(arr) {
  if (arr.length <= 1) return [arr];
  const out = [];
  arr.forEach((x, i) => {
    const rest = arr.slice(0, i).concat(arr.slice(i + 1));
    for (const p of permutations(rest)) out.push([x, ...p]);
  });
  return out;
}

function hanoi(n, from, to, via, moves) {
  if (n === 0) return;
  hanoi(n - 1, from, via, to, moves);
  moves.push(from + to);
  hanoi(n - 1, via, to, from, moves);
}

function isEven(n) { return n === 0 ? true : isOdd(n - 1); }
function isOdd(n) { return n === 0 ? false : isEven(n - 1); }

function floodFill(grid, r, c, from, to) {
  if (r < 0 || c < 0 || r >= grid.length || c >= grid[0].length) return 0;
  if (grid[r][c] !== from) return 0;
  grid[r][c] = to;
  return 1 + floodFill(grid, r + 1, c, from, to) + floodFill(grid, r - 1, c, from, to)
    + floodFill(grid, r, c + 1, from, to) + floodFill(grid, r, c - 1, from, to);
}

const n = Number(process.argv[2] || 7);
console.log("queens", n, queens(n));
console.log("perms", permutations([1, 2, 3, 4, 5]).length);
const moves = [];
hanoi(9, "A", "C", "B", moves);
console.log("hanoi moves", moves.length);
console.log("even 301?", isEven(301));
const grid = Array.from({ length: 24 }, (_, r) => Array.from({ length: 24 }, (_, c) => ((r * 7 + c * 3) % 11 === 0 ? 1 : 0)));
console.log("flood", floodFill(grid, 1, 1, 0, 2));
