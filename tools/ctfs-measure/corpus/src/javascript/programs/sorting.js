// Classic sorting algorithms over a deterministic pseudo-random array.
function lcg(seed) {
  let s = seed >>> 0;
  return function () {
    s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
    return s / 4294967296;
  };
}

function insertionSort(a) {
  for (let i = 1; i < a.length; i++) {
    const key = a[i];
    let j = i - 1;
    while (j >= 0 && a[j] > key) {
      a[j + 1] = a[j];
      j--;
    }
    a[j + 1] = key;
  }
  return a;
}

function mergeSort(a) {
  if (a.length <= 1) return a;
  const mid = a.length >> 1;
  const left = mergeSort(a.slice(0, mid));
  const right = mergeSort(a.slice(mid));
  const out = [];
  let i = 0, j = 0;
  while (i < left.length && j < right.length) {
    if (left[i] <= right[j]) out.push(left[i++]);
    else out.push(right[j++]);
  }
  while (i < left.length) out.push(left[i++]);
  while (j < right.length) out.push(right[j++]);
  return out;
}

function partition(a, lo, hi) {
  const pivot = a[hi];
  let i = lo;
  for (let j = lo; j < hi; j++) {
    if (a[j] < pivot) {
      const t = a[i]; a[i] = a[j]; a[j] = t;
      i++;
    }
  }
  const t = a[i]; a[i] = a[hi]; a[hi] = t;
  return i;
}

function quickSort(a, lo = 0, hi = a.length - 1) {
  if (lo < hi) {
    const p = partition(a, lo, hi);
    quickSort(a, lo, p - 1);
    quickSort(a, p + 1, hi);
  }
  return a;
}

function siftDown(a, start, end) {
  let root = start;
  while (2 * root + 1 <= end) {
    let child = 2 * root + 1;
    let swap = root;
    if (a[swap] < a[child]) swap = child;
    if (child + 1 <= end && a[swap] < a[child + 1]) swap = child + 1;
    if (swap === root) return;
    const t = a[root]; a[root] = a[swap]; a[swap] = t;
    root = swap;
  }
}

function heapSort(a) {
  for (let start = (a.length - 2) >> 1; start >= 0; start--) siftDown(a, start, a.length - 1);
  for (let end = a.length - 1; end > 0; end--) {
    const t = a[end]; a[end] = a[0]; a[0] = t;
    siftDown(a, 0, end - 1);
  }
  return a;
}

function isSorted(a) {
  for (let i = 1; i < a.length; i++) if (a[i - 1] > a[i]) return false;
  return true;
}

const n = Number(process.argv[2] || 600);
const rand = lcg(42);
const data = Array.from({ length: n }, () => Math.floor(rand() * 10000));
const algos = { insertionSort, mergeSort, quickSort, heapSort };
for (const [name, fn] of Object.entries(algos)) {
  const sorted = fn(data.slice());
  console.log(name, isSorted(sorted), sorted[0], sorted[sorted.length - 1]);
}
