# Recursion-heavy workloads: naive fib, Ackermann, permutations, n-queens, Hanoi, merge sort.
def fib(n) = n < 2 ? n : fib(n - 1) + fib(n - 2)

def ackermann(m, n)
  if m.zero? then n + 1
  elsif n.zero? then ackermann(m - 1, 1)
  else ackermann(m - 1, ackermann(m, n - 1))
  end
end

def permutations(items)
  return [items] if items.size <= 1
  items.each_with_index.flat_map do |x, i|
    rest = items[0...i] + items[(i + 1)..]
    permutations(rest).map { |p| [x] + p }
  end
end

def queens(n, row = 0, cols = [], count = 0)
  return count + 1 if row == n
  n.times do |c|
    next if cols.each_with_index.any? { |cc, r| cc == c || (cc - c).abs == row - r }
    count = queens(n, row + 1, cols + [c], count)
  end
  count
end

def hanoi(n, from, to, via, moves)
  return if n.zero?
  hanoi(n - 1, from, via, to, moves)
  moves << [from, to]
  hanoi(n - 1, via, to, from, moves)
end

def merge_sort(a)
  return a if a.size <= 1
  mid = a.size / 2
  l = merge_sort(a[0...mid])
  r = merge_sort(a[mid..])
  out = []
  until l.empty? || r.empty?
    out << (l.first <= r.first ? l.shift : r.shift)
  end
  out + l + r
end

puts fib(18)
puts ackermann(2, 3)
puts permutations([1, 2, 3, 4, 5]).size
puts queens(6)
moves = []
hanoi(10, :a, :c, :b, moves)
puts moves.size
puts merge_sort(Array.new(600) { |i| (i * 7919) % 1009 }).first(5).inspect
