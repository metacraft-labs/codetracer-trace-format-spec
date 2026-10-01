// Backtracking Sudoku solver (recursion + nested loops + early returns).
const SIZE = 9;

function isValid(board, row, col, num) {
  for (let c = 0; c < SIZE; c++) {
    if (board[row][c] === num) return false;
  }
  for (let r = 0; r < SIZE; r++) {
    if (board[r][col] === num) return false;
  }
  const br = Math.floor(row / 3) * 3;
  const bc = Math.floor(col / 3) * 3;
  for (let r = br; r < br + 3; r++) {
    for (let c = bc; c < bc + 3; c++) {
      if (board[r][c] === num) return false;
    }
  }
  return true;
}

function findEmpty(board) {
  for (let r = 0; r < SIZE; r++) {
    for (let c = 0; c < SIZE; c++) {
      if (board[r][c] === 0) return [r, c];
    }
  }
  return null;
}

function solve(board) {
  const pos = findEmpty(board);
  if (pos === null) return true;
  const [row, col] = pos;
  for (let num = 1; num <= 9; num++) {
    if (isValid(board, row, col, num)) {
      board[row][col] = num;
      if (solve(board)) return true;
      board[row][col] = 0;
    }
  }
  return false;
}

function parse(text) {
  const rows = [];
  for (let i = 0; i < 9; i++) {
    const row = [];
    for (let j = 0; j < 9; j++) {
      const ch = text[i * 9 + j];
      row.push(ch === "." ? 0 : Number(ch));
    }
    rows.push(row);
  }
  return rows;
}

function render(board) {
  return board.map((row) => row.join("")).join("\n");
}

const puzzles = [
  "53..7....6..195....98....6.8...6...34..8.3..17...2...6.6....28....419..5....8..79",
  "..3.2.6..9..3.5..1..18.64....81.29..7.......8..67.82....26.95..8..2.3..9..5.1.3..",
  "4.....8.5.3..........7......2.....6.....8.4......1.......6.3.7.5..2.....1.4......",
];
const limit = Number(process.argv[2] || puzzles.length);
for (let i = 0; i < limit; i++) {
  const b = parse(puzzles[i]);
  const ok = solve(b);
  console.log(`puzzle ${i}: ${ok ? "solved" : "unsolvable"}`);
  console.log(render(b));
}
