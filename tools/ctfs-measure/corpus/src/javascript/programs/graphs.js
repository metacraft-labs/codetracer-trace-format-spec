// Graph algorithms: BFS, DFS (recursive), Dijkstra with a binary heap,
// Kruskal with union-find, topological sort.
class MinHeap {
  constructor() { this.items = []; }
  get size() { return this.items.length; }
  push(prio, value) {
    const items = this.items;
    items.push([prio, value]);
    let i = items.length - 1;
    while (i > 0) {
      const p = (i - 1) >> 1;
      if (items[p][0] <= items[i][0]) break;
      [items[p], items[i]] = [items[i], items[p]];
      i = p;
    }
  }
  pop() {
    const items = this.items;
    const top = items[0];
    const last = items.pop();
    if (items.length > 0) {
      items[0] = last;
      let i = 0;
      for (;;) {
        const l = 2 * i + 1, r = l + 1;
        let m = i;
        if (l < items.length && items[l][0] < items[m][0]) m = l;
        if (r < items.length && items[r][0] < items[m][0]) m = r;
        if (m === i) break;
        [items[m], items[i]] = [items[i], items[m]];
        i = m;
      }
    }
    return top;
  }
}

function makeGraph(n, seed) {
  let s = seed;
  const rnd = () => ((s = (s * 48271) % 2147483647) / 2147483647);
  const adj = Array.from({ length: n }, () => []);
  const edges = [];
  for (let u = 0; u < n; u++) {
    const deg = 1 + Math.floor(rnd() * 4);
    for (let k = 0; k < deg; k++) {
      const v = Math.floor(rnd() * n);
      if (v === u) continue;
      const w = 1 + Math.floor(rnd() * 20);
      adj[u].push({ to: v, w });
      adj[v].push({ to: u, w });
      edges.push([w, u, v]);
    }
  }
  return { adj, edges };
}

function bfs(adj, src) {
  const dist = new Array(adj.length).fill(-1);
  const queue = [src];
  dist[src] = 0;
  let head = 0;
  while (head < queue.length) {
    const u = queue[head++];
    for (const { to } of adj[u]) {
      if (dist[to] === -1) {
        dist[to] = dist[u] + 1;
        queue.push(to);
      }
    }
  }
  return dist;
}

function dfsCount(adj, u, seen) {
  seen.add(u);
  let count = 1;
  for (const e of adj[u]) {
    if (!seen.has(e.to)) count += dfsCount(adj, e.to, seen);
  }
  return count;
}

function dijkstra(adj, src) {
  const dist = new Map();
  const heap = new MinHeap();
  heap.push(0, src);
  while (heap.size > 0) {
    const [d, u] = heap.pop();
    if (dist.has(u)) continue;
    dist.set(u, d);
    for (const { to, w } of adj[u]) {
      if (!dist.has(to)) heap.push(d + w, to);
    }
  }
  return dist;
}

function kruskal(n, edges) {
  const parent = Array.from({ length: n }, (_, i) => i);
  const rank = new Array(n).fill(0);
  const find = (x) => {
    while (parent[x] !== x) {
      parent[x] = parent[parent[x]];
      x = parent[x];
    }
    return x;
  };
  const sorted = edges.slice().sort((a, b) => a[0] - b[0]);
  let total = 0, used = 0;
  for (const [w, u, v] of sorted) {
    const ru = find(u), rv = find(v);
    if (ru === rv) continue;
    if (rank[ru] < rank[rv]) parent[ru] = rv;
    else if (rank[ru] > rank[rv]) parent[rv] = ru;
    else { parent[rv] = ru; rank[ru]++; }
    total += w;
    used++;
  }
  return { total, used };
}

function topoSort(n) {
  const deps = Array.from({ length: n }, (_, i) => (i > 1 ? [i % 7, (i * 3) % i] : []));
  const indeg = new Array(n).fill(0);
  const out = Array.from({ length: n }, () => []);
  deps.forEach((ds, i) => ds.forEach((d) => { if (d < i) { out[d].push(i); indeg[i]++; } }));
  const ready = [];
  indeg.forEach((d, i) => { if (d === 0) ready.push(i); });
  const order = [];
  while (ready.length) {
    const u = ready.shift();
    order.push(u);
    for (const v of out[u]) if (--indeg[v] === 0) ready.push(v);
  }
  return order;
}

const N = Number(process.argv[2] || 400);
const { adj, edges } = makeGraph(N, 12345);
const d = bfs(adj, 0);
console.log("bfs max depth", Math.max(...d));
console.log("component size", dfsCount(adj, 0, new Set()));
const dj = dijkstra(adj, 0);
let far = 0;
for (const v of dj.values()) far = Math.max(far, v);
console.log("dijkstra farthest", far);
console.log("mst", kruskal(N, edges));
console.log("topo length", topoSort(N).length);
