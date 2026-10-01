"""Generator/iterator pipelines, closures, decorators, comprehensions and lambdas over a streaming workload."""
import functools
import itertools


def counted(fn):
    calls = 0

    @functools.wraps(fn)
    def wrapper(*args, **kwargs):
        nonlocal calls
        calls += 1
        return fn(*args, **kwargs)

    wrapper.calls = lambda: calls
    return wrapper


def source(n, seed):
    x = seed
    for _ in range(n):
        x = (x * 6364136223846793005 + 1442695040888963407) % (1 << 64)
        yield x >> 40


def windowed(it, k):
    buf = []
    for v in it:
        buf.append(v)
        if len(buf) == k:
            yield tuple(buf)
            buf.pop(0)


@counted
def score(win):
    return sum(w * (i + 1) for i, w in enumerate(win)) % 9973


def make_filter(threshold):
    def keep(v):
        return v > threshold
    return keep


def collatz_len(n):
    steps = 0
    while n != 1:
        n = n // 2 if n % 2 == 0 else 3 * n + 1
        steps += 1
    return steps


def main():
    keep = make_filter(5000)
    scored = (score(w) for w in windowed(source(3000, 11), 4))
    kept = list(filter(keep, scored))
    print("kept", len(kept), "calls", score.calls())
    buckets = {k: len(list(g)) for k, g in itertools.groupby(sorted(kept), key=lambda v: v // 1000)}
    print("buckets", buckets)
    lens = [collatz_len(n) for n in range(1, 400)]
    print("collatz max", max(lens), lens.index(max(lens)) + 1)
    pairs = [(a, b) for a in range(1, 40) for b in range(a, 40) if (a * a + b * b) ** 0.5 % 1 == 0]
    print("pythagorean", pairs[:5], len(pairs))
    acc = functools.reduce(lambda a, b: (a * 31 + b) % 1000003, itertools.chain.from_iterable(pairs), 7)
    print("acc", acc)


main()
