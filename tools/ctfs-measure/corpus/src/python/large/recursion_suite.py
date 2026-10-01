"""Recursion-heavy workloads: n-queens backtracking, permutations, Ackermann, memo fib, Hanoi, subset sum."""
import functools
import sys

sys.setrecursionlimit(10000)


def queens(n):
    sols = 0
    cols, d1, d2 = set(), set(), set()

    def place(r):
        nonlocal sols
        if r == n:
            sols += 1
            return
        for c in range(n):
            if c in cols or (r - c) in d1 or (r + c) in d2:
                continue
            cols.add(c); d1.add(r - c); d2.add(r + c)
            place(r + 1)
            cols.remove(c); d1.remove(r - c); d2.remove(r + c)

    place(0)
    return sols


def permutations(xs):
    if len(xs) <= 1:
        return [xs]
    out = []
    for i, x in enumerate(xs):
        for p in permutations(xs[:i] + xs[i + 1:]):
            out.append([x] + p)
    return out


def ackermann(m, n):
    if m == 0:
        return n + 1
    if n == 0:
        return ackermann(m - 1, 1)
    return ackermann(m - 1, ackermann(m, n - 1))


@functools.lru_cache(maxsize=None)
def fib(n):
    return n if n < 2 else fib(n - 1) + fib(n - 2)


def naive_fib(n):
    return n if n < 2 else naive_fib(n - 1) + naive_fib(n - 2)


def hanoi(n, a, b, c, moves):
    if n == 0:
        return
    hanoi(n - 1, a, c, b, moves)
    moves.append((a, c))
    hanoi(n - 1, b, a, c, moves)


def subset_sum(xs, target, i=0):
    if target == 0:
        return True
    if i == len(xs) or target < 0:
        return False
    return subset_sum(xs, target - xs[i], i + 1) or subset_sum(xs, target, i + 1)


def main():
    print("queens7", queens(7))
    print("perms", len(permutations(list(range(6)))))
    print("ack", ackermann(2, 30))
    print("fib", fib(300))
    print("naive", naive_fib(18))
    moves = []
    hanoi(11, "A", "B", "C", moves)
    print("hanoi", len(moves))
    print("subset", subset_sum([31, 7, 19, 23, 5, 11, 29, 3, 17, 13, 41, 37], 1000))


main()
