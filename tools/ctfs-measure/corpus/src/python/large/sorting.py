"""Sorting algorithm implementations compared against sorted()."""
import random


def quicksort(a, lo=0, hi=None):
    if hi is None:
        hi = len(a) - 1
    while lo < hi:
        p = a[(lo + hi) // 2]
        i, j = lo, hi
        while i <= j:
            while a[i] < p:
                i += 1
            while a[j] > p:
                j -= 1
            if i <= j:
                a[i], a[j] = a[j], a[i]
                i += 1
                j -= 1
        if j - lo < hi - i:
            quicksort(a, lo, j)
            lo = i
        else:
            quicksort(a, i, hi)
            hi = j
    return a


def mergesort(a):
    if len(a) <= 1:
        return a
    m = len(a) // 2
    l, r = mergesort(a[:m]), mergesort(a[m:])
    out, i, j = [], 0, 0
    while i < len(l) and j < len(r):
        if l[i] <= r[j]:
            out.append(l[i]); i += 1
        else:
            out.append(r[j]); j += 1
    out.extend(l[i:]); out.extend(r[j:])
    return out


def heapsort(a):
    n = len(a)

    def sift(i, n):
        while True:
            l, big = 2 * i + 1, i
            if l < n and a[l] > a[big]:
                big = l
            if l + 1 < n and a[l + 1] > a[big]:
                big = l + 1
            if big == i:
                return
            a[i], a[big] = a[big], a[i]
            i = big

    for i in range(n // 2 - 1, -1, -1):
        sift(i, n)
    for end in range(n - 1, 0, -1):
        a[0], a[end] = a[end], a[0]
        sift(0, end)
    return a


def insertion(a):
    for i in range(1, len(a)):
        x, j = a[i], i - 1
        while j >= 0 and a[j] > x:
            a[j + 1] = a[j]
            j -= 1
        a[j + 1] = x
    return a


def main():
    rng = random.Random(42)
    for n in (50, 300, 1200):
        data = [rng.randrange(10000) for _ in range(n)]
        ref = sorted(data)
        assert quicksort(list(data)) == ref
        assert mergesort(list(data)) == ref
        assert heapsort(list(data)) == ref
        if n <= 300:
            assert insertion(list(data)) == ref
        print("ok", n)


main()
