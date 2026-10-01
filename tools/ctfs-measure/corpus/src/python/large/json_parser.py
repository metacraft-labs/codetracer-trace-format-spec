"""Hand-written JSON tokenizer + recursive-descent parser + serializer, run on generated documents."""
import random


class ParseError(Exception):
    pass


class Lexer:
    def __init__(self, text):
        self.text = text
        self.pos = 0
        self.n = len(text)

    def skip_ws(self):
        while self.pos < self.n and self.text[self.pos] in " \t\r\n":
            self.pos += 1

    def peek(self):
        self.skip_ws()
        if self.pos >= self.n:
            return ""
        return self.text[self.pos]

    def expect(self, ch):
        if self.peek() != ch:
            raise ParseError(f"expected {ch!r} at {self.pos}")
        self.pos += 1

    def string(self):
        self.expect('"')
        out = []
        while True:
            c = self.text[self.pos]
            if c == '"':
                self.pos += 1
                return "".join(out)
            if c == "\\":
                nxt = self.text[self.pos + 1]
                out.append({"n": "\n", "t": "\t", '"': '"', "\\": "\\"}.get(nxt, nxt))
                self.pos += 2
            else:
                out.append(c)
                self.pos += 1

    def number(self):
        start = self.pos
        if self.text[self.pos] == "-":
            self.pos += 1
        is_float = False
        while self.pos < self.n and (self.text[self.pos].isdigit() or self.text[self.pos] in ".eE+-"):
            if self.text[self.pos] in ".eE":
                is_float = True
            self.pos += 1
        lit = self.text[start:self.pos]
        return float(lit) if is_float else int(lit)


def parse_value(lx):
    c = lx.peek()
    if c == "{":
        return parse_object(lx)
    if c == "[":
        return parse_array(lx)
    if c == '"':
        return lx.string()
    if c == "t":
        lx.pos += 4
        return True
    if c == "f":
        lx.pos += 5
        return False
    if c == "n":
        lx.pos += 4
        return None
    return lx.number()


def parse_object(lx):
    lx.expect("{")
    obj = {}
    if lx.peek() == "}":
        lx.pos += 1
        return obj
    while True:
        key = lx.string()
        lx.expect(":")
        obj[key] = parse_value(lx)
        if lx.peek() == ",":
            lx.pos += 1
            continue
        lx.expect("}")
        return obj


def parse_array(lx):
    lx.expect("[")
    arr = []
    if lx.peek() == "]":
        lx.pos += 1
        return arr
    while True:
        arr.append(parse_value(lx))
        if lx.peek() == ",":
            lx.pos += 1
            continue
        lx.expect("]")
        return arr


def dump(v):
    if isinstance(v, dict):
        return "{" + ",".join('"%s":%s' % (k, dump(x)) for k, x in v.items()) + "}"
    if isinstance(v, list):
        return "[" + ",".join(dump(x) for x in v) + "]"
    if isinstance(v, str):
        return '"' + v.replace("\\", "\\\\").replace('"', '\\"') + '"'
    if v is True:
        return "true"
    if v is False:
        return "false"
    if v is None:
        return "null"
    return repr(v)


def gen(rng, depth):
    r = rng.random()
    if depth > 3 or r < 0.3:
        k = rng.randrange(5)
        if k == 0:
            return rng.randrange(-1000, 1000)
        if k == 1:
            return round(rng.uniform(-10, 10), 3)
        if k == 2:
            return "".join(rng.choice("abcdefgh xyz\"") for _ in range(rng.randrange(1, 9)))
        if k == 3:
            return rng.random() < 0.5
        return None
    if r < 0.65:
        return [gen(rng, depth + 1) for _ in range(rng.randrange(0, 5))]
    return {"k%d" % i: gen(rng, depth + 1) for i in range(rng.randrange(0, 5))}


def count_nodes(v):
    if isinstance(v, dict):
        return 1 + sum(count_nodes(x) for x in v.values())
    if isinstance(v, list):
        return 1 + sum(count_nodes(x) for x in v)
    return 1


def main():
    rng = random.Random(1234)
    total = 0
    for i in range(40):
        doc = gen(rng, 0)
        text = dump(doc)
        parsed = parse_value(Lexer(text))
        assert dump(parsed) == text
        total += count_nodes(parsed)
    print("nodes", total)


main()
