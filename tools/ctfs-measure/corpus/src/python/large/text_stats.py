"""Stdlib-heavy text processing: re, collections, string, csv, textwrap, itertools, statistics."""
import csv
import io
import itertools
import re
import statistics
import string
import textwrap
from collections import Counter, OrderedDict, defaultdict

WORDS = ("alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho "
         "sigma tau upsilon phi chi psi omega river stone cloud forest ember signal vector matrix").split()


def make_corpus(n_lines):
    out = []
    state = 17
    for i in range(n_lines):
        words = []
        for j in range(8 + (i % 7)):
            state = (state * 1103515245 + 12345) & 0x7FFFFFFF
            words.append(WORDS[state % len(WORDS)])
        line = " ".join(words).capitalize() + ("." if i % 3 else "!")
        if i % 5 == 0:
            line += f" id={i} score={state % 1000}"
        out.append(line)
    return out


TOKEN = re.compile(r"[A-Za-z]+|\d+|[.!=]")
KV = re.compile(r"(\w+)=(\d+)")


def analyze(lines):
    counts = Counter()
    bigrams = Counter()
    lengths = []
    kv = defaultdict(list)
    for line in lines:
        toks = [t.lower() for t in TOKEN.findall(line) if t.isalpha()]
        counts.update(toks)
        bigrams.update(zip(toks, toks[1:]))
        lengths.append(len(line))
        for k, v in KV.findall(line):
            kv[k].append(int(v))
    return counts, bigrams, lengths, kv


def to_csv(counts):
    buf = io.StringIO()
    w = csv.writer(buf)
    w.writerow(["word", "count", "upper", "vowels"])
    for word, c in counts.most_common():
        w.writerow([word, c, word.upper(), sum(ch in "aeiou" for ch in word)])
    buf.seek(0)
    rows = list(csv.DictReader(buf))
    return rows


def caesar(text, k):
    table = str.maketrans(string.ascii_lowercase, string.ascii_lowercase[k:] + string.ascii_lowercase[:k])
    return text.translate(table)


def main():
    lines = make_corpus(1500)
    counts, bigrams, lengths, kv = analyze(lines)
    rows = to_csv(counts)
    print("rows", len(rows), "top", counts.most_common(3))
    print("bigrams", bigrams.most_common(2))
    print("len mean", statistics.mean(lengths), "median", statistics.median(lengths))
    print("scores", statistics.pstdev(kv["score"]))
    od = OrderedDict(sorted(counts.items()))
    groups = {k: len(list(g)) for k, g in itertools.groupby(sorted(od), key=lambda w: w[0])}
    print("groups", groups)
    para = " ".join(lines[:200])
    wrapped = textwrap.wrap(caesar(para.lower(), 3), width=60)
    print(len(wrapped), wrapped[0])


main()
