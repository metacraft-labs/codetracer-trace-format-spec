# Graph algorithms over a generated weighted graph: binary heap Dijkstra, BFS,
# Kruskal with union-find, topological sort, strongly connected components.
class MinHeap
  def initialize = @a = []
  def empty? = @a.empty?
  def size = @a.size

  def push(item)
    @a << item
    i = @a.size - 1
    while i > 0
      parent = (i - 1) / 2
      break if @a[parent][0] <= @a[i][0]
      @a[parent], @a[i] = @a[i], @a[parent]
      i = parent
    end
  end

  def pop
    top = @a.first
    last = @a.pop
    unless @a.empty?
      @a[0] = last
      i = 0
      loop do
        l = 2 * i + 1
        r = l + 1
        m = i
        m = l if l < @a.size && @a[l][0] < @a[m][0]
        m = r if r < @a.size && @a[r][0] < @a[m][0]
        break if m == i
        @a[m], @a[i] = @a[i], @a[m]
        i = m
      end
    end
    top
  end
end

class UnionFind
  def initialize(n)
    @parent = (0...n).to_a
    @rank = Array.new(n, 0)
  end

  def find(x)
    @parent[x] = find(@parent[x]) if @parent[x] != x
    @parent[x]
  end

  def union(a, b)
    ra, rb = find(a), find(b)
    return false if ra == rb
    ra, rb = rb, ra if @rank[ra] < @rank[rb]
    @parent[rb] = ra
    @rank[ra] += 1 if @rank[ra] == @rank[rb]
    true
  end
end

N = 300
rng = Random.new(42)
edges = []
adj = Hash.new { |h, k| h[k] = [] }
N.times do |u|
  4.times do
    v = rng.rand(N)
    next if v == u
    w = rng.rand(1..50)
    edges << [w, u, v]
    adj[u] << [v, w]
  end
end

def dijkstra(adj, src, n)
  dist = Array.new(n, Float::INFINITY)
  dist[src] = 0
  heap = MinHeap.new
  heap.push([0, src])
  until heap.empty?
    d, u = heap.pop
    next if d > dist[u]
    adj[u].each do |v, w|
      nd = d + w
      if nd < dist[v]
        dist[v] = nd
        heap.push([nd, v])
      end
    end
  end
  dist
end

def bfs(adj, src)
  seen = { src => 0 }
  queue = [src]
  until queue.empty?
    u = queue.shift
    adj[u].each do |v, _|
      next if seen.key?(v)
      seen[v] = seen[u] + 1
      queue << v
    end
  end
  seen
end

def kruskal(edges, n)
  uf = UnionFind.new(n)
  edges.sort.each_with_object([]) do |(w, u, v), mst|
    mst << w if uf.union(u, v)
  end.sum
end

def tarjan(adj, n)
  index = 0
  idx = {}
  low = {}
  stack = []
  on = {}
  comps = 0
  strong = lambda do |v|
    idx[v] = low[v] = index
    index += 1
    stack.push(v)
    on[v] = true
    adj[v].each do |w, _|
      if !idx.key?(w)
        strong.(w)
        low[v] = [low[v], low[w]].min
      elsif on[w]
        low[v] = [low[v], idx[w]].min
      end
    end
    if low[v] == idx[v]
      comps += 1
      loop do
        w = stack.pop
        on[w] = false
        break if w == v
      end
    end
  end
  n.times { |v| strong.(v) unless idx.key?(v) }
  comps
end

def topo(n, rng)
  dag = Hash.new { |h, k| h[k] = [] }
  indeg = Array.new(n, 0)
  (n * 2).times do
    a, b = rng.rand(n), rng.rand(n)
    a, b = b, a if a > b
    next if a == b
    dag[a] << b
    indeg[b] += 1
  end
  ready = (0...n).select { |v| indeg[v].zero? }
  order = []
  until ready.empty?
    v = ready.pop
    order << v
    dag[v].each { |w| ready << w if (indeg[w] -= 1).zero? }
  end
  order.size
end

puts dijkstra(adj, 0, N).reject(&:infinite?).sum
puts bfs(adj, 0).values.max
puts kruskal(edges, N)
puts tarjan(adj, N)
puts topo(N, rng)
