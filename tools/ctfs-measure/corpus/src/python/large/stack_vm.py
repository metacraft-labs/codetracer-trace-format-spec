"""Tiny compiler + stack VM: tokenize/parse arithmetic programs, compile to bytecode, interpret with dispatch loop."""
import re

TOK = re.compile(r"\s*(?:(\d+)|(\w+)|(.))")


def tokenize(src):
    out = []
    for num, name, op in TOK.findall(src):
        if num:
            out.append(("num", int(num)))
        elif name:
            out.append(("name", name))
        elif op.strip():
            out.append(("op", op))
    out.append(("eof", None))
    return out


class Parser:
    def __init__(self, toks):
        self.toks = toks
        self.i = 0

    def peek(self):
        return self.toks[self.i]

    def take(self):
        t = self.toks[self.i]
        self.i += 1
        return t

    def program(self):
        stmts = []
        while self.peek()[0] != "eof":
            stmts.append(self.stmt())
        return stmts

    def stmt(self):
        kind, val = self.peek()
        if kind == "name" and val == "while":
            self.take()
            cond = self.expr()
            self.take()  # {
            body = []
            while self.peek() != ("op", "}"):
                body.append(self.stmt())
            self.take()
            return ("while", cond, body)
        if kind == "name" and val == "print":
            self.take()
            e = self.expr()
            self.take()  # ;
            return ("print", e)
        name = self.take()[1]
        self.take()  # =
        e = self.expr()
        self.take()  # ;
        return ("assign", name, e)

    def expr(self):
        left = self.term()
        while self.peek()[0] == "op" and self.peek()[1] in "+-<":
            op = self.take()[1]
            left = (op, left, self.term())
        return left

    def term(self):
        left = self.atom()
        while self.peek()[0] == "op" and self.peek()[1] in "*%":
            op = self.take()[1]
            left = (op, left, self.atom())
        return left

    def atom(self):
        kind, val = self.take()
        if kind == "num":
            return ("const", val)
        if kind == "name":
            return ("var", val)
        e = self.expr()
        self.take()
        return e


def compile_expr(e, code):
    tag = e[0]
    if tag == "const":
        code.append(("PUSH", e[1]))
    elif tag == "var":
        code.append(("LOAD", e[1]))
    else:
        compile_expr(e[1], code)
        compile_expr(e[2], code)
        code.append(("BIN", tag))


def compile_stmts(stmts, code):
    for s in stmts:
        if s[0] == "assign":
            compile_expr(s[2], code)
            code.append(("STORE", s[1]))
        elif s[0] == "print":
            compile_expr(s[1], code)
            code.append(("PRINT", None))
        else:
            start = len(code)
            compile_expr(s[1], code)
            jmp = len(code)
            code.append(("JZ", None))
            compile_stmts(s[2], code)
            code.append(("JMP", start))
            code[jmp] = ("JZ", len(code))


def run(code):
    env, stack, pc, out = {}, [], 0, []
    while pc < len(code):
        op, arg = code[pc]
        pc += 1
        if op == "PUSH":
            stack.append(arg)
        elif op == "LOAD":
            stack.append(env.get(arg, 0))
        elif op == "STORE":
            env[arg] = stack.pop()
        elif op == "BIN":
            b, a = stack.pop(), stack.pop()
            if arg == "+":
                stack.append(a + b)
            elif arg == "-":
                stack.append(a - b)
            elif arg == "*":
                stack.append(a * b)
            elif arg == "%":
                stack.append(a % b)
            else:
                stack.append(1 if a < b else 0)
        elif op == "JZ":
            if stack.pop() == 0:
                pc = arg
        elif op == "JMP":
            pc = arg
        elif op == "PRINT":
            out.append(stack.pop())
    return out


SRC = """
i = 0; acc = 0;
while i < 600 {
  j = 0;
  while j < 5 { acc = (acc + i * j + 7) % 100003; j = j + 1; }
  i = i + 1;
}
print acc;
n = 2; count = 0;
while n < 120 {
  d = 2; prime = 1;
  while d * d < (n + 1) { if_ = n % d; prime = prime * (0 < if_); d = d + 1; }
  count = count + prime; n = n + 1;
}
print count;
"""


def main():
    stmts = Parser(tokenize(SRC)).program()
    code = []
    compile_stmts(stmts, code)
    print(len(code), run(code))


main()
