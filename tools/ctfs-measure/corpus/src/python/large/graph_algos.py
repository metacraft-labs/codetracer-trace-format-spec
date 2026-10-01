"""Graph algorithms: Dijkstra (heapq), BFS, DFS topological sort, union-find Kruskal."""
import heapq
import random
from collections import deque, defaultdict


def make_graph(rng, n, m):
    edges = []
    for _ in range(m):
        a, b = rng.randrange(n), rng.randrange(n)
        if a != b:
            edges.append((a, b, rng.randrange(1, 50)))
    adj = defaultdict(list)
    for a, b, w in edges:
        adj[a].append((b, w))
        adj[b].append((a, w))
    return edges, adj


def dijkstra(adj, src, n):
    dist = [float("inf")] * n
    dist[src] = 0
    pq = [(0, src)]
    while pq:
        d, u = heapq.heappop(pq)
        if d > dist[u]:
            continue
        for v, w in adj[u]:
            nd = d + w
            if nd < dist[v]:
                dist[v] = nd
                heapq.heappush(pq, (nd, v))
    return dist


def bfs_levels(adj, src):
    seen = {src: 0}
    q = deque([src])
    while q:
        u = q.popleft()
        for v, _ in adj[u]:
            if v not in seen:
                seen[v] = seen[u] + 1
                q.append(v)
    return seen


class DSU:
    def __init__(self, n):
        self.p = list(range(n))
        self.r = [0] * n

    def find(self, x):
        while self.p[x] != x:
            self.p[x] = self.p[self.p[x]]
            x = self.p[x]
        return x

    def union(self, a, b):
        a, b = self.find(a), self.find(b)
        if a == b:
            return False
        if self.r[a] < self.r[b]:
            a, b = b, a
        self.p[b] = a
        if self.r[a] == self.r[b]:
            self.r[a] += 1
        return True


def kruskal(edges, n):
    dsu = DSU(n)
    total = 0
    for a, b, w in sorted(edges, key=lambda e: e[2]):
        if dsu.union(a, b):
            total += w
    return total


def topo_dfs(n, rng):
    dag = defaultdict(list)
    for _ in range(n * 3):
        a, b = sorted((rng.randrange(n), rng.randrange(n)))
        if a != b:
            dag[a].append(b)
    order, state = [], [0] * n

    def visit(u):
        state[u] = 1
        for v in dag[u]:
            if state[v] == 0:
                visit(v)
        state[u] = 2
        order.append(u)

    for u in range(n):
        if state[u] == 0:
            visit(u)
    return order[::-1]


def main():
    rng = random.Random(7)
    n = 120
    edges, adj = make_graph(rng, n, 400)
    dsum = 0
    for s in range(0, n, 20):
        dist = dijkstra(adj, s, n)
        dsum += sum(d for d in dist if d != float("inf"))
    levels = bfs_levels(adj, 0)
    print("dijkstra", dsum, "bfs-depth", max(levels.values()), "mst", kruskal(edges, n))
    print("topo", topo_dfs(150, rng)[:5])


main()
