# Larger realistic GDScript workload for the trace-format corpus:
# an inventory/economy simulation with classes, dictionaries, arrays, sorting,
# string formatting and a priority-queue pathfinder on a grid.
extends MainLoop

class Item:
	var name: String
	var price: float
	var qty: int
	func _init(n, p, q):
		name = n
		price = p
		qty = q
	func value() -> float:
		return price * qty

var rng_state := 12345

func rnd(n: int) -> int:
	rng_state = (rng_state * 1103515245 + 12345) % 2147483648
	return rng_state % n

func make_inventory(count: int) -> Array:
	var names = ["sword", "shield", "potion", "arrow", "helm", "boots", "ring", "scroll"]
	var inv = []
	for i in range(count):
		var it = Item.new("%s_%d" % [names[rnd(names.size())], i], float(rnd(1000)) / 10.0, rnd(20) + 1)
		inv.append(it)
	return inv

func total_value(inv: Array) -> float:
	var t = 0.0
	for it in inv:
		t += it.value()
	return t

func bubble_sort(a: Array) -> Array:
	var b = a.duplicate()
	var n = b.size()
	for i in range(n):
		var swapped = false
		for j in range(n - i - 1):
			if b[j] > b[j + 1]:
				var tmp = b[j]
				b[j] = b[j + 1]
				b[j + 1] = tmp
				swapped = true
		if not swapped:
			break
	return b

func market_tick(inv: Array, prices: Dictionary) -> int:
	var trades = 0
	for it in inv:
		var kind = it.name.split("_")[0]
		if not prices.has(kind):
			prices[kind] = it.price
		var delta = (rnd(21) - 10) / 100.0
		prices[kind] = max(0.1, prices[kind] * (1.0 + delta))
		if prices[kind] > it.price and it.qty > 0:
			it.qty -= 1
			trades += 1
		elif prices[kind] < it.price * 0.8:
			it.qty += 1
	return trades

func dijkstra(grid: Array, w: int, h: int) -> int:
	var dist = {}
	var frontier = [[0, 0, 0]]
	dist[Vector2i(0, 0)] = 0
	while frontier.size() > 0:
		var best = 0
		for k in range(frontier.size()):
			if frontier[k][0] < frontier[best][0]:
				best = k
		var cur = frontier[best]
		frontier.remove_at(best)
		var d = cur[0]
		var x = cur[1]
		var y = cur[2]
		if x == w - 1 and y == h - 1:
			return d
		for off in [Vector2i(1, 0), Vector2i(-1, 0), Vector2i(0, 1), Vector2i(0, -1)]:
			var nx = x + off.x
			var ny = y + off.y
			if nx < 0 or ny < 0 or nx >= w or ny >= h:
				continue
			var nd = d + grid[ny * w + nx]
			var key = Vector2i(nx, ny)
			if not dist.has(key) or nd < dist[key]:
				dist[key] = nd
				frontier.append([nd, nx, ny])
	return -1

func fib(n: int) -> int:
	if n < 2:
		return n
	return fib(n - 1) + fib(n - 2)

func _init():
	var inv = make_inventory(60)
	var prices = {}
	var trades = 0
	for day in range(15):
		trades += market_tick(inv, prices)
	var values = []
	for it in inv:
		values.append(int(it.value()))
	var sorted = bubble_sort(values)
	var w = 12
	var h = 12
	var grid = []
	for i in range(w * h):
		grid.append(rnd(9) + 1)
	var path = dijkstra(grid, w, h)
	var f = fib(15)
	var report = "trades=%d top=%d path=%d fib=%d total=%.1f" % [trades, sorted[sorted.size() - 1], path, f, total_value(inv)]
	print("CT_BIG_SIM=" + report)

func _process(_delta):
	return true
