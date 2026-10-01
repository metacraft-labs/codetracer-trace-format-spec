# Tight numeric loops: matrix multiply, Mandelbrot, sieve, Collatz, LCG statistics.
def matmul(a, b)
  n = a.size
  m = b[0].size
  k = b.size
  c = Array.new(n) { Array.new(m, 0) }
  i = 0
  while i < n
    j = 0
    while j < m
      s = 0
      t = 0
      while t < k
        s += a[i][t] * b[t][j]
        t += 1
      end
      c[i][j] = s
      j += 1
    end
    i += 1
  end
  c
end

def mandelbrot(w, h, iters)
  rows = []
  h.times do |y|
    row = +''
    w.times do |x|
      cr = x * 3.0 / w - 2.0
      ci = y * 2.0 / h - 1.0
      zr = zi = 0.0
      n = 0
      while n < iters && zr * zr + zi * zi < 4.0
        zr, zi = zr * zr - zi * zi + cr, 2 * zr * zi + ci
        n += 1
      end
      row << (n == iters ? '#' : '.')
    end
    rows << row
  end
  rows
end

def sieve(n)
  flags = Array.new(n + 1, true)
  flags[0] = flags[1] = false
  (2..Math.sqrt(n)).each do |i|
    next unless flags[i]
    (i * i).step(n, i) { |j| flags[j] = false }
  end
  flags.count(true)
end

def collatz_longest(limit)
  best = [0, 0]
  (1..limit).each do |start|
    n = start
    len = 1
    until n == 1
      n = n.even? ? n / 2 : 3 * n + 1
      len += 1
    end
    best = [len, start] if len > best[0]
  end
  best
end

a = Array.new(14) { |i| Array.new(14) { |j| (i * j) % 7 - 3 } }
puts matmul(a, a).flatten.sum
puts mandelbrot(36, 14, 30).join("\n")
puts sieve(3000)
puts collatz_longest(400).inspect
seed = 12345
hist = Array.new(10, 0)
2000.times do
  seed = (seed * 1_103_515_245 + 12_345) % 2**31
  hist[seed % 10] += 1
end
puts hist.inspect
