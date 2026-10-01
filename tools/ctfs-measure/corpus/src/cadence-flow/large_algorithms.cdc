access(all) fun fib(_ n: Int): Int {
    if n < 2 {
        return n
    }
    return fib(n - 1) + fib(n - 2)
}

access(all) fun bubble_sort(_ input: [Int]): [Int] {
    var xs = input
    var i = 0
    while i < xs.length {
        var j = 0
        while j < xs.length - i - 1 {
            if xs[j] > xs[j + 1] {
                let t = xs[j]
                xs[j] = xs[j + 1]
                xs[j + 1] = t
            }
            j = j + 1
        }
        i = i + 1
    }
    return xs
}

access(all) fun manhattan(_ xs: [Int], _ ys: [Int]): Int {
    var total = 0
    var idx = 0
    while idx < xs.length {
        var dx = xs[idx]
        if dx < 0 { dx = 0 - dx }
        var dy = ys[idx]
        if dy < 0 { dy = 0 - dy }
        total = total + dx + dy
        idx = idx + 1
    }
    return total
}

access(all) fun word_counts(_ words: [String]): {String: Int} {
    let counts: {String: Int} = {}
    for w in words {
        let c = counts[w] ?? 0
        counts[w] = c + 1
    }
    return counts
}

access(all) fun main(): Int {
    let f = fib(14)
    let sorted = bubble_sort([9, 3, 7, 1, 8, 2, 6, 4, 5, 0, 15, 11, 13, 12, 14, 10])
    var pxs: [Int] = []
    var pys: [Int] = []
    var k = -10
    while k < 10 {
        pxs.append(k)
        pys.append(k * 2 - 3)
        k = k + 1
    }
    let d = manhattan(pxs, pys)
    let wc = word_counts(["a", "b", "a", "c", "b", "a", "flow", "cadence", "flow"])
    let result = f + sorted[0] + sorted[15] + d + (wc["a"] ?? 0)
    return result
}
